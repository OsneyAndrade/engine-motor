import argparse
import ctypes
import functools
import gc
import hashlib
import io
import json
import math
import mmap
import multiprocessing as mp
import os
import platform
import shutil
import stat
import statistics
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request
from array import array
import numpy as np
from compression import zstd

KIB = 1 << 10
MIB = 1 << 20
URL_BASE = "https://cdn.kernel.org/pub/linux/kernel/v6.x/"
SEMENTE_GEAR = 20261103
SEMENTE_MASCARAS = 1234567
SEMENTE_SANIDADE = 424242
SEMENTE_SINTETICO = 20261103
HASH_BYTES = 32
PRB_MES = 5e15
CHUNKERS = {
    "fixo_4K": ("fixo", 4 * KIB),
    "fixo_8K": ("fixo", 8 * KIB),
    "fixo_64K": ("fixo", 64 * KIB),
    "fixo_1M": ("fixo", MIB),
    "cdc_8K": ("cdc", 8 * KIB),
    "cdc_64K": ("cdc", 64 * KIB),
    "cdc_1M": ("cdc", MIB),
}
CHUNKERS_Z19 = ("cdc_64K", "cdc_1M")
CONJUNTOS = {
    "A_arvore": {"tipo": "arvore", "versoes": ["6.12.114", "6.12.115"]},
    "A_tar": {"tipo": "tar", "versoes": ["6.12.114", "6.12.115"]},
    "B_arvore": {"tipo": "arvore", "versoes": ["6.12.115", "6.13.12"]},
    "B_tar": {"tipo": "tar", "versoes": ["6.12.115", "6.13.12"]},
    "sintetico": {"tipo": "sintetico", "versoes": ["backup1", "backup2"]},
}
VERSOES_KERNEL = ["6.12.114", "6.12.115", "6.13.12"]
CP = zstd.CompressionParameter
DP = zstd.DecompressionParameter

LIB = None
GEAR = None
MASCARAS = {}
DADOS = None
AMOSTRA19 = 4
FONTES = {}


def compilar(pasta):
    fonte = os.path.join(os.path.dirname(os.path.abspath(__file__)), "cdc.c")
    so = os.path.join(pasta, "libcdc.so")
    subprocess.run(["gcc", "-O3", "-shared", "-fPIC", "-o", so, fonte], check=True)
    lib = ctypes.CDLL(so)
    lib.fastcdc_cortar.restype = ctypes.c_int64
    lib.fastcdc_cortar.argtypes = [
        ctypes.c_void_p,
        ctypes.c_int64,
        ctypes.c_int64,
        ctypes.c_int64,
        ctypes.c_int64,
        ctypes.c_uint64,
        ctypes.c_uint64,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_void_p,
        ctypes.c_int64,
    ]
    return lib


def mascaras(alvo):
    b = int(math.log2(alvo))
    perm = np.random.default_rng(SEMENTE_MASCARAS).permutation(np.arange(16, 64)).tolist()

    def montar(n):
        v = 0
        for p in perm[:n]:
            v |= 1 << p
        return v

    return montar(b + 2), montar(b - 2)


def iniciar():
    global LIB, GEAR, MASCARAS
    pasta = tempfile.mkdtemp(prefix="cdc-")
    LIB = compilar(pasta)
    GEAR = np.random.default_rng(SEMENTE_GEAR).integers(0, 2**64, size=256, dtype=np.uint64)
    MASCARAS = {cfg[1]: mascaras(cfg[1]) for cfg in CHUNKERS.values() if cfg[0] == "cdc"}
    return pasta


def endereco(buf):
    return np.frombuffer(buf, dtype=np.uint8).ctypes.data


def chunkear(buf, n, cfg):
    tipo, alvo = cfg
    if n == 0:
        return np.zeros(0, np.int64), np.zeros(0, np.int64)
    if tipo == "fixo":
        offs = np.arange(0, n, alvo, dtype=np.int64)
        return offs, np.minimum(alvo, n - offs)
    minimo = alvo // 4
    if n <= minimo:
        return np.zeros(1, np.int64), np.full(1, n, np.int64)
    cap = n // minimo + 2
    offs = np.empty(cap, np.int64)
    tams = np.empty(cap, np.int64)
    ms, ml = MASCARAS[alvo]
    k = LIB.fastcdc_cortar(endereco(buf), n, minimo, alvo, alvo * 8, ms, ml, GEAR.ctypes.data, offs.ctypes.data, tams.ctypes.data, cap)
    if k < 0:
        raise RuntimeError("capacidade insuficiente")
    return offs[:k], tams[:k]


def hash_chunk(v):
    return hashlib.blake2b(v, digest_size=HASH_BYTES).digest()


def sha256_arquivo(caminho):
    h = hashlib.sha256()
    with open(caminho, "rb") as f:
        while True:
            b = f.read(8 * MIB)
            if not b:
                break
            h.update(b)
    return h.hexdigest()


def carregar_json(caminho):
    if os.path.exists(caminho):
        with open(caminho) as f:
            return json.load(f)
    return {}


def salvar_json(caminho, obj):
    tmp = caminho + ".tmp"
    with open(tmp, "w") as f:
        json.dump(obj, f, indent=1)
    os.replace(tmp, caminho)


def log(*a):
    print(*a, file=sys.stderr, flush=True)


def sha_esperado(dados, nome):
    pasta = os.path.join(dados, "sha256sums")
    arq = os.path.join(pasta, "sha256sums.asc")
    if not os.path.exists(arq):
        os.makedirs(pasta, exist_ok=True)
        with urllib.request.urlopen(URL_BASE + "sha256sums.asc", timeout=60) as r, open(arq, "wb") as f:
            f.write(r.read())
    with open(arq) as f:
        for linha in f:
            p = linha.split()
            if len(p) == 2 and p[1] == nome:
                return p[0]
    raise RuntimeError("sha256 nao encontrado para " + nome)


def baixar(url, destino):
    with urllib.request.urlopen(url, timeout=120) as r, open(destino, "wb") as f:
        shutil.copyfileobj(r, f, 1 << 20)


def preparar_versao(dados, versao):
    t0 = time.time()
    nome = "linux-%s.tar.xz" % versao
    pasta_dl = os.path.join(dados, "dl-" + versao)
    os.makedirs(pasta_dl, exist_ok=True)
    arq = os.path.join(pasta_dl, nome)
    esperado = sha_esperado(dados, nome)
    if not os.path.exists(arq):
        baixar(URL_BASE + nome, arq)
    obtido = sha256_arquivo(arq)
    info = {
        "versao": versao,
        "arquivo_xz": nome,
        "bytes_xz": os.path.getsize(arq),
        "sha256_esperado": esperado,
        "sha256_obtido": obtido,
        "sha256_confere": esperado == obtido,
        "url": URL_BASE + nome,
    }
    if esperado != obtido:
        raise RuntimeError("sha256 diverge para " + nome)
    pasta_arv = os.path.join(dados, "arv-" + versao)
    os.makedirs(pasta_arv, exist_ok=False)
    with tarfile.open(arq, "r:xz") as t:
        t.extractall(pasta_arv, filter="data")
    raiz = os.path.join(pasta_arv, "linux-" + versao)
    entradas = []
    for dp, dns, fns in os.walk(raiz):
        for n in dns + fns:
            entradas.append(os.path.relpath(os.path.join(dp, n), raiz))
    entradas.sort()
    pasta_tar = os.path.join(dados, "tar-" + versao)
    os.makedirs(pasta_tar, exist_ok=False)
    caminho_tar = os.path.join(pasta_tar, "linux.tar")
    arquivos = []
    n_dirs = n_links = n_outros = 0
    bytes_dados = 0
    alocado = 0
    with tarfile.open(caminho_tar, "w", format=tarfile.GNU_FORMAT) as tf:
        for rel in entradas:
            p = os.path.join(raiz, rel)
            st = os.lstat(p)
            ti = tarfile.TarInfo("linux/" + rel)
            ti.uid = ti.gid = 0
            ti.uname = ti.gname = ""
            ti.mtime = 0
            if stat.S_ISDIR(st.st_mode):
                ti.type = tarfile.DIRTYPE
                ti.mode = 0o755
                tf.addfile(ti)
                n_dirs += 1
            elif stat.S_ISLNK(st.st_mode):
                ti.type = tarfile.SYMTYPE
                ti.linkname = os.readlink(p)
                ti.mode = 0o777
                tf.addfile(ti)
                n_links += 1
            elif stat.S_ISREG(st.st_mode):
                with open(p, "rb") as f:
                    dados_arq = f.read()
                ti.size = len(dados_arq)
                ti.mode = 0o755 if st.st_mode & 0o100 else 0o644
                tf.addfile(ti, io.BytesIO(dados_arq))
                off = tf.offset - ((len(dados_arq) + 511) // 512) * 512
                arquivos.append({"c": rel, "t": len(dados_arq), "s": hashlib.sha256(dados_arq).hexdigest(), "o": off})
                bytes_dados += len(dados_arq)
                alocado += st.st_blocks * 512
            else:
                n_outros += 1
    offsets_ok = True
    with open(caminho_tar, "rb") as f:
        mm = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
        for x in arquivos:
            if x["t"] and hashlib.sha256(mm[x["o"] : x["o"] + x["t"]]).hexdigest() != x["s"]:
                offsets_ok = False
        mm.close()
    shutil.rmtree(pasta_arv)
    info.update(
        {
            "offsets_no_tar_conferem_com_sha256_dos_arquivos": offsets_ok,
            "arvore_extraida_apagada_apos_gerar_tar": True,
            "bytes_alocados_arvore_tmpfs": alocado,
            "n_arquivos": len(arquivos),
            "n_arquivos_vazios": sum(1 for a in arquivos if a["t"] == 0),
            "n_diretorios": n_dirs,
            "n_symlinks": n_links,
            "n_outros_ignorados": n_outros,
            "bytes_dados_arvore": bytes_dados,
            "bytes_tar": os.path.getsize(caminho_tar),
            "sha256_tar": sha256_arquivo(caminho_tar),
            "tempo_preparo_s": time.time() - t0,
        }
    )
    with open(os.path.join(dados, "manifesto-%s.json" % versao), "w") as f:
        json.dump({"info": info, "arquivos": arquivos}, f)
    return info


def preparar_sintetico(dados, silesia):
    t0 = time.time()
    pasta = os.path.join(dados, "sint")
    os.makedirs(pasta, exist_ok=False)
    nomes = sorted(n for n in os.listdir(silesia) if os.path.isfile(os.path.join(silesia, n)))
    partes = []
    for n in nomes:
        with open(os.path.join(silesia, n), "rb") as f:
            partes.append(f.read())
    b1 = np.frombuffer(b"".join(partes), dtype=np.uint8).copy()
    n = len(b1)
    rng = np.random.default_rng(SEMENTE_SINTETICO)
    b2 = b1.copy()
    alvo_edicao = n // 100
    acumulado = 0
    n_edicoes = 0
    while acumulado < alvo_edicao:
        tam = int(rng.integers(1 * KIB, 4 * KIB + 1))
        pos = int(rng.integers(0, n - tam))
        b2[pos : pos + tam] = np.frombuffer(rng.bytes(tam), dtype=np.uint8)
        acumulado += tam
        n_edicoes += 1
    pos_ins = sorted(int(x) for x in rng.integers(0, n + 1, size=40))
    tam_ins = [int(x) for x in rng.integers(20, 2001, size=40)]
    pedacos = []
    ant = 0
    for p, t in zip(pos_ins, tam_ins):
        pedacos.append(b2[ant:p].tobytes())
        pedacos.append(rng.bytes(t))
        ant = p
    pedacos.append(b2[ant:].tobytes())
    novos = n // 100
    pedacos.append(rng.bytes(novos))
    b2 = b"".join(pedacos)
    caminhos = {}
    for rotulo, dado in (("backup1", b1.tobytes()), ("backup2", b2)):
        p = os.path.join(pasta, rotulo + ".bin")
        with open(p, "wb") as f:
            f.write(dado)
        caminhos[rotulo] = {"caminho": p, "bytes": len(dado), "sha256": hashlib.sha256(dado).hexdigest()}
    info = {
        "rotulo": "SINTETICO",
        "silesia_arquivos_em_ordem": nomes,
        "semente": SEMENTE_SINTETICO,
        "edicoes_in_place": {"n": n_edicoes, "bytes": acumulado, "fracao": acumulado / n, "tamanho_min_max": [KIB, 4 * KIB], "conteudo": "bytes aleatorios (incompressiveis)"},
        "insercoes": {"n": 40, "bytes": sum(tam_ins), "tamanho_min_max": [20, 2000], "conteudo": "bytes aleatorios"},
        "anexo_novo": {"bytes": novos, "fracao": novos / n, "conteudo": "bytes aleatorios"},
        "arquivos": caminhos,
        "tempo_preparo_s": time.time() - t0,
    }
    with open(os.path.join(dados, "manifesto-sint.json"), "w") as f:
        json.dump(info, f)
    return info


def fase_preparar(a):
    dados = os.path.abspath(a.dados)
    os.makedirs(dados, exist_ok=True)
    esperados = {v: sha_esperado(dados, "linux-%s.tar.xz" % v) for v in VERSOES_KERNEL}
    log("sha256 esperados:", esperados)
    sint = None
    m_sint = os.path.join(dados, "manifesto-sint.json")
    if os.path.exists(m_sint):
        with open(m_sint) as f:
            sint = json.load(f)
    elif a.silesia:
        sint = preparar_sintetico(dados, os.path.abspath(a.silesia))
    res = {}
    for v in a.versoes.split(","):
        info = preparar_versao(dados, v)
        res[v] = info
        log("preparo ok", v, info["n_arquivos"], info["bytes_tar"], info["bytes_alocados_arvore_tmpfs"], "%.0fs" % info["tempo_preparo_s"])
    saida = carregar_json(a.saida)
    anterior = saida.get("dados", {}).get("versoes_kernel", {})
    anterior.update(res)
    res = anterior
    saida["dados"] = {"versoes_kernel": res, "sintetico": sint, "diretorio_listagem": URL_BASE}
    salvar_json(a.saida, saida)
    log("preparo concluido:", {v: (i["n_arquivos"], i["bytes_tar"]) for v, i in res.items()})


class Fonte:
    def __init__(self, dados, tipo, ident):
        self.tipo = tipo
        self.ident = ident
        if tipo == "arvore":
            with open(os.path.join(dados, "manifesto-%s.json" % ident)) as f:
                man = json.load(f)
            f = open(os.path.join(dados, "tar-" + ident, "linux.tar"), "rb")
            self.mm = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
            self.itens = [x for x in man["arquivos"] if x["t"] > 0]
            self.n = len(self.itens)
            self.logico = sum(x["t"] for x in self.itens)
            self.n_vazios = len(man["arquivos"]) - self.n
        else:
            if tipo == "tar":
                with open(os.path.join(dados, "manifesto-%s.json" % ident)) as f:
                    info = json.load(f)["info"]
                caminho = os.path.join(dados, "tar-" + ident, "linux.tar")
                sha = info["sha256_tar"]
            else:
                with open(os.path.join(dados, "manifesto-sint.json")) as f:
                    info = json.load(f)["arquivos"][ident]
                caminho = info["caminho"]
                sha = info["sha256"]
            f = open(caminho, "rb")
            self.mm = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
            self.itens = [{"c": caminho, "t": len(self.mm), "s": sha}]
            self.n = 1
            self.logico = len(self.mm)
            self.n_vazios = 0

    def ler(self, i):
        if self.tipo == "arvore":
            x = self.itens[i]
            return memoryview(self.mm)[x["o"] : x["o"] + x["t"]]
        return self.mm


def fontes_do_conjunto(conj):
    if conj in FONTES:
        return FONTES[conj]
    d = CONJUNTOS[conj]
    tipo = d["tipo"]
    fs = [Fonte(DADOS, tipo, v) for v in d["versoes"]]
    FONTES[conj] = fs
    return fs


def categorias(conj):
    f1, f2 = fontes_do_conjunto(conj)
    mapa = {x["c"]: x["s"] for x in f1.itens}
    cats = []
    for x in f2.itens:
        s = mapa.get(x["c"])
        cats.append("novo" if s is None else ("igual" if s == x["s"] else "alterado"))
    return cats


def varrer_chunks(conj, nome_chunker, com_z19):
    t0 = time.time()
    cfg = CHUNKERS[nome_chunker]
    fontes = fontes_do_conjunto(conj)
    zc = zstd.compress
    zd = zstd.decompress
    b2 = hashlib.blake2b
    dic = {}
    u_ver = array("b")
    u_obj = array("q")
    u_off = array("q")
    u_tam = array("q")
    u_c3 = array("q")
    u_c19 = array("q")
    recs = [array("q"), array("q")]
    ini = [array("q"), array("q")]
    n_chunks = [0, 0]
    vs_v1 = 0
    dup_total = 0
    codec_ok = True
    amostra = AMOSTRA19 if com_z19 else 0
    for ver in (0, 1):
        f = fontes[ver]
        rec = recs[ver]
        for i in range(f.n):
            dados = f.ler(i)
            n = len(dados)
            offs, tams = chunkear(dados, n, cfg)
            mv = memoryview(dados)
            ini[ver].append(len(rec))
            n_chunks[ver] += len(offs)
            for o, s in zip(offs.tolist(), tams.tolist()):
                v = mv[o : o + s]
                h = b2(v, digest_size=HASH_BYTES).digest()
                u = dic.get(h)
                if u is None:
                    u = len(u_ver)
                    dic[h] = u
                    u_ver.append(ver)
                    u_obj.append(i)
                    u_off.append(o)
                    u_tam.append(s)
                    c = zc(v, level=3)
                    if zd(c) != v:
                        codec_ok = False
                    u_c3.append(len(c))
                    if amostra and h[0] % amostra == 0:
                        c19 = zc(v, level=19)
                        if zd(c19) != v:
                            codec_ok = False
                        u_c19.append(len(c19))
                    else:
                        u_c19.append(-1)
                elif ver == 1:
                    dup_total += s
                    if u_ver[u] == 0:
                        vs_v1 += s
                rec.append(u)
    tempo_varredura = time.time() - t0
    exato = True
    falhas = []
    f2 = fontes[1]
    rec = recs[1]
    fim_rec = len(rec)

    @functools.lru_cache(maxsize=32)
    def ler_cache(ver, i):
        return fontes[ver].ler(i)

    for j in range(f2.n):
        h = hashlib.sha256()
        a = ini[1][j]
        b = ini[1][j + 1] if j + 1 < f2.n else fim_rec
        for k in range(a, b):
            u = rec[k]
            d = ler_cache(u_ver[u], u_obj[u])
            h.update(memoryview(d)[u_off[u] : u_off[u] + u_tam[u]])
        if h.hexdigest() != f2.itens[j]["s"]:
            exato = False
            falhas.append(f2.itens[j]["c"])
            if len(falhas) > 5:
                break
    n_u = len(u_ver)
    stored_raw = sum(u_tam)
    stored_z3 = sum(u_c3)
    r = {
        "conjunto": conj,
        "chunker": nome_chunker,
        "logico_v1": fontes[0].logico,
        "logico_v2": fontes[1].logico,
        "n_chunks_v1": n_chunks[0],
        "n_chunks_v2": n_chunks[1],
        "n_chunks_unicos": n_u,
        "n_chunks_unicos_v1": sum(1 for v in u_ver if v == 0),
        "bytes_unicos": stored_raw,
        "bytes_v2_ja_em_v1": vs_v1,
        "pct_v2_ja_em_v1": 100.0 * vs_v1 / fontes[1].logico,
        "bytes_v2_duplicados_total": dup_total,
        "pct_v2_duplicados_total": 100.0 * dup_total / fontes[1].logico,
        "armazenado_bruto": stored_raw,
        "armazenado_z3": stored_z3,
        "razao_bruta": (fontes[0].logico + fontes[1].logico) / stored_raw,
        "razao_z3": (fontes[0].logico + fontes[1].logico) / stored_z3,
        "referencias_v1": n_chunks[0],
        "referencias_v2": n_chunks[1],
        "media_chunk_v1": fontes[0].logico / max(n_chunks[0], 1),
        "media_chunk_unico": stored_raw / max(n_u, 1),
        "bit_exato": exato and codec_ok,
        "reconstrucao_v2_ok": exato,
        "codec_roundtrip_ok": codec_ok,
        "falhas_reconstrucao": falhas,
        "tempo_varredura_s": tempo_varredura,
        "tempo_total_s": time.time() - t0,
    }
    if amostra:
        s3 = sum(c3 for c3, c19 in zip(u_c3, u_c19) if c19 >= 0)
        s19 = sum(c19 for c19 in u_c19 if c19 >= 0)
        n_s = sum(1 for c19 in u_c19 if c19 >= 0)
        r["z19"] = {
            "amostra_1_em": amostra,
            "n_chunks_amostrados": n_s,
            "bytes_amostra_z3": s3,
            "bytes_amostra_z19": s19,
            "razao_z19_sobre_z3_na_amostra": s19 / s3 if s3 else None,
            "armazenado_z19_estimado": stored_z3 * s19 / s3 if s3 else None,
            "razao_z19_estimada": (fontes[0].logico + fontes[1].logico) / (stored_z3 * s19 / s3) if s3 else None,
        }
    if fontes[0].tipo == "arvore":
        cats = categorias(conj)
        marg = {"igual": 0, "alterado": 0, "novo": 0}
        logico_cat = {"igual": 0, "alterado": 0, "novo": 0}
        n_cat = {"igual": 0, "alterado": 0, "novo": 0}
        for j, c in enumerate(cats):
            logico_cat[c] += f2.itens[j]["t"]
            n_cat[c] += 1
        for u in range(n_u):
            if u_ver[u] == 1:
                marg[cats[u_obj[u]]] += u_c3[u]
        r["v2_por_categoria"] = {"arquivos": n_cat, "bytes_logicos": logico_cat, "custo_marginal_z3": marg}
    return r


def tarefa_arquivo_inteiro(conj):
    t0 = time.time()
    fontes = fontes_do_conjunto(conj)
    b2 = hashlib.blake2b
    dic = {}
    c3_por_hash = {}
    todos = [[], []]
    primeiro = {}
    codec_ok = True
    for ver in (0, 1):
        f = fontes[ver]
        for i in range(f.n):
            d = f.ler(i)
            h = b2(d, digest_size=HASH_BYTES).digest()
            todos[ver].append(h)
            if h not in dic:
                dic[h] = (ver, i, len(d))
                c = zstd.compress(d, level=3)
                if zstd.decompress(c) != d:
                    codec_ok = False
                c3_por_hash[h] = len(c)
    h1 = set(todos[0])
    f2 = fontes[1]
    vs_v1 = sum(f2.itens[j]["t"] for j, h in enumerate(todos[1]) if h in h1)
    ja_visto = set(todos[0])
    dup_total = 0
    for j, h in enumerate(todos[1]):
        if h in ja_visto:
            dup_total += f2.itens[j]["t"]
        ja_visto.add(h)
    exato = True
    for j, h in enumerate(todos[1]):
        ver, i, n = dic[h]
        d = fontes[ver].ler(i)
        if hashlib.sha256(d).hexdigest() != f2.itens[j]["s"]:
            exato = False
    cats = categorias(conj)
    marg = {"igual": 0, "alterado": 0, "novo": 0}
    visto = set(todos[0])
    for j, h in enumerate(todos[1]):
        if h not in visto:
            marg[cats[j]] += c3_por_hash[h]
            visto.add(h)
    logico = [fontes[0].logico, fontes[1].logico]
    z3_sem_dedup = sum(c3_por_hash[h] for h in todos[0]) + sum(c3_por_hash[h] for h in todos[1])
    z3_com_dedup = sum(c3_por_hash.values())
    bruto_unico = sum(v[2] for v in dic.values())
    return {
        "conjunto": conj,
        "chunker": "arquivo_inteiro",
        "logico_v1": logico[0],
        "logico_v2": logico[1],
        "n_arquivos_v1": fontes[0].n,
        "n_arquivos_v2": fontes[1].n,
        "n_arquivos_vazios_ignorados": [fontes[0].n_vazios, fontes[1].n_vazios],
        "n_unicos": len(dic),
        "bytes_unicos": bruto_unico,
        "bytes_v2_ja_em_v1": vs_v1,
        "pct_v2_ja_em_v1": 100.0 * vs_v1 / logico[1],
        "bytes_v2_duplicados_total": dup_total,
        "pct_v2_duplicados_total": 100.0 * dup_total / logico[1],
        "armazenado_bruto_sem_dedup": sum(logico),
        "armazenado_bruto_com_dedup": bruto_unico,
        "armazenado_z3_sem_dedup": z3_sem_dedup,
        "armazenado_z3_com_dedup": z3_com_dedup,
        "razao_z3_sem_dedup": sum(logico) / z3_sem_dedup,
        "razao_z3_com_dedup": sum(logico) / z3_com_dedup,
        "razao_bruta_com_dedup": sum(logico) / bruto_unico,
        "referencias_v1": fontes[0].n,
        "referencias_v2": fontes[1].n,
        "bit_exato": exato and codec_ok,
        "reconstrucao_v2_ok": exato,
        "codec_roundtrip_ok": codec_ok,
        "v2_por_categoria": {
            "arquivos": {c: cats.count(c) for c in ("igual", "alterado", "novo")},
            "bytes_logicos": {c: sum(f2.itens[j]["t"] for j, x in enumerate(cats) if x == c) for c in ("igual", "alterado", "novo")},
            "custo_marginal_z3": marg,
        },
        "tempo_total_s": time.time() - t0,
    }


def comprimir_fluxo(buffers, opcoes, opcoes_dec=None):
    c = zstd.ZstdCompressor(options=opcoes)
    saidas = []
    passo = 64 * MIB
    for mm in buffers:
        mv = memoryview(mm)
        for o in range(0, len(mv), passo):
            s = c.compress(mv[o : o + passo])
            if s:
                saidas.append(s)
    saidas.append(c.flush())
    total = sum(len(s) for s in saidas)
    d = zstd.ZstdDecompressor(options=opcoes_dec)
    h = hashlib.sha256()
    for s in saidas:
        h.update(d.decompress(s))
    return total, h.hexdigest()


def tarefa_base_fluxo(conj, ver):
    t0 = time.time()
    f = fontes_do_conjunto(conj)[ver]
    total, sha = comprimir_fluxo([f.mm], {CP.compression_level: 3})
    return {
        "conjunto": conj,
        "tarefa": "base_fluxo",
        "versao_idx": ver,
        "logico": f.logico,
        "z3": total,
        "razao_z3": f.logico / total,
        "bit_exato": sha == f.itens[0]["s"],
        "tempo_total_s": time.time() - t0,
    }


def executar_tarefa(t):
    if t["tipo"] == "chunk":
        return varrer_chunks(t["conj"], t["chunker"], t["z19"])
    if t["tipo"] == "arquivo":
        return tarefa_arquivo_inteiro(t["conj"])
    return tarefa_base_fluxo(t["conj"], t["ver"])


def custo_tarefa(t):
    if t["tipo"] == "chunk":
        base = 1.0
        if t["z19"]:
            base += 100
        if t["conj"] == "sintetico":
            base *= 0.3
        return base + (0.1 if t["chunker"].startswith("fixo_4K") else 0)
    return 0.5


def fase_analise(a):
    global DADOS, AMOSTRA19
    DADOS = os.path.abspath(a.dados)
    AMOSTRA19 = a.amostra19
    pasta = iniciar()
    saida = carregar_json(a.saida)
    conjuntos = a.conjuntos.split(",") if a.conjuntos else list(CONJUNTOS)
    tarefas = []
    for conj in conjuntos:
        for nome in CHUNKERS:
            tarefas.append({"tipo": "chunk", "conj": conj, "chunker": nome, "z19": nome in CHUNKERS_Z19 and a.amostra19 > 0})
        if CONJUNTOS[conj]["tipo"] == "arvore":
            tarefas.append({"tipo": "arquivo", "conj": conj})
        else:
            for ver in (0, 1):
                tarefas.append({"tipo": "base", "conj": conj, "ver": ver})
    for conj in conjuntos:
        fontes_do_conjunto(conj)
    tarefas.sort(key=custo_tarefa, reverse=True)
    ctx = mp.get_context("fork")
    t0 = time.time()
    resultados = []
    with ctx.Pool(a.workers) as pool:
        for r in pool.imap_unordered(executar_tarefa, tarefas, chunksize=1):
            resultados.append(r)
            log("[analise] %d/%d %s %s %.0fs (decorrido %.0fs) bit_exato=%s" % (len(resultados), len(tarefas), r.get("conjunto"), r.get("chunker", r.get("tarefa")), r["tempo_total_s"], time.time() - t0, r.get("bit_exato")))
    saida.setdefault("conjuntos", {})
    for conj in conjuntos:
        c = {"tipo": CONJUNTOS[conj]["tipo"], "versoes": CONJUNTOS[conj]["versoes"], "estrategias": {}}
        fs = fontes_do_conjunto(conj)
        c["logico_v1"] = fs[0].logico
        c["logico_v2"] = fs[1].logico
        bases = {}
        for r in resultados:
            if r.get("conjunto") != conj:
                continue
            if r.get("tarefa") == "base_fluxo":
                bases[r["versao_idx"]] = r
            elif r["chunker"] == "arquivo_inteiro":
                c["arquivo_inteiro"] = r
            else:
                c["estrategias"][r["chunker"]] = r
        if bases:
            c["fluxo_sem_dedup_z3"] = {
                "z3_v1": bases[0]["z3"],
                "z3_v2": bases[1]["z3"],
                "armazenado_z3": bases[0]["z3"] + bases[1]["z3"],
                "razao_z3": (fs[0].logico + fs[1].logico) / (bases[0]["z3"] + bases[1]["z3"]),
                "bit_exato": bases[0]["bit_exato"] and bases[1]["bit_exato"],
            }
        saida["conjuntos"][conj] = c
    saida["analise"] = {"workers": a.workers, "amostra_z19_1_em": a.amostra19, "tempo_s": time.time() - t0, "data": time.strftime("%Y-%m-%d %H:%M:%S")}
    violacoes = [{"conjunto": r.get("conjunto"), "estrategia": r.get("chunker", r.get("tarefa"))} for r in resultados if not r.get("bit_exato")]
    saida["bit_exatidao_violada"] = violacoes
    salvar_json(a.saida, saida)
    log("analise concluida em %.0fs; violacoes de bit-exatidao: %d" % (time.time() - t0, len(violacoes)))
    shutil.rmtree(pasta, ignore_errors=True)


def fase_long(a):
    global DADOS
    DADOS = os.path.abspath(a.dados)
    saida = carregar_json(a.saida)
    for conj in [c for c in CONJUNTOS if CONJUNTOS[c]["tipo"] != "arvore"]:
        t0 = time.time()
        fs = fontes_do_conjunto(conj)
        total, sha = comprimir_fluxo(
            [fs[0].mm, fs[1].mm],
            {CP.compression_level: 3, CP.window_log: 31, CP.enable_long_distance_matching: 1},
            {DP.window_log_max: 31},
        )
        esperado = hashlib.sha256()
        for f in fs:
            esperado.update(f.mm)
        l = fs[0].logico + fs[1].logico
        saida["conjuntos"][conj]["fluxo_z3_long31_concatenado"] = {
            "armazenado": total,
            "razao": l / total,
            "bit_exato": sha == esperado.hexdigest(),
            "tempo_s": time.time() - t0,
            "nota": "referencia extra: zstd-3 com janela 2 GiB e long-distance matching sobre v1+v2 concatenados; sem indice por chunk",
        }
        log("[long31]", conj, total, sha == esperado.hexdigest(), "%.0fs" % (time.time() - t0))
    salvar_json(a.saida, saida)


def delta_lote(args):
    conj, lote = args
    f1, f2 = fontes_do_conjunto(conj)
    res = {"n": 0, "bytes_v2": 0, "patch": 0, "z3_sozinho": 0, "ok": True}
    for j2, j1 in lote:
        ref = f1.ler(j1)
        novo = f2.ler(j2)
        wl = min(27, max(21, math.ceil(math.log2(len(ref) + len(novo)))))
        d = zstd.ZstdDict(bytes(ref), is_raw=True)
        c = zstd.compress(novo, options={CP.compression_level: 3, CP.window_log: wl, CP.enable_long_distance_matching: 1}, zstd_dict=d.as_prefix)
        if zstd.decompress(c, options={DP.window_log_max: wl}, zstd_dict=d.as_prefix) != novo:
            res["ok"] = False
        res["n"] += 1
        res["bytes_v2"] += len(novo)
        res["patch"] += len(c)
        res["z3_sozinho"] += len(zstd.compress(novo, level=3))
    return res


def referencia_cli(conj, pares):
    f1, f2 = fontes_do_conjunto(conj)
    linhas = []
    pasta = tempfile.mkdtemp(prefix="delta-cli-")
    for j2, j1 in pares:
        ref = os.path.join(pasta, "ref.bin")
        novo = os.path.join(pasta, "novo.bin")
        with open(ref, "wb") as f:
            f.write(f1.ler(j1))
        with open(novo, "wb") as f:
            f.write(f2.ler(j2))
        r = subprocess.run(["zstd", "--patch-from=" + ref, "-3", "--no-check", "-q", "-c", novo], capture_output=True)
        ref_d = f1.ler(j1)
        nv = f2.ler(j2)
        wl = min(27, max(21, math.ceil(math.log2(len(ref_d) + len(nv)))))
        d = zstd.ZstdDict(bytes(ref_d), is_raw=True)
        c = zstd.compress(nv, options={CP.compression_level: 3, CP.window_log: wl, CP.enable_long_distance_matching: 1}, zstd_dict=d.as_prefix)
        linhas.append((len(r.stdout), len(c)))
    shutil.rmtree(pasta)
    return linhas


def fase_delta(a):
    global DADOS
    DADOS = os.path.abspath(a.dados)
    pasta = iniciar()
    saida = carregar_json(a.saida)
    saida.setdefault("delta", {})
    ctx = mp.get_context("fork")
    for conj in [c for c in CONJUNTOS if CONJUNTOS[c]["tipo"] == "arvore"]:
        t0 = time.time()
        f1, f2 = fontes_do_conjunto(conj)
        cats = categorias(conj)
        idx1 = {x["c"]: i for i, x in enumerate(f1.itens)}
        alterados = [(j, idx1[f2.itens[j]["c"]]) for j, c in enumerate(cats) if c == "alterado"]
        lotes = [(conj, alterados[i : i + 200]) for i in range(0, len(alterados), 200)]
        agg = {"n": 0, "bytes_v2": 0, "patch": 0, "z3_sozinho": 0, "ok": True}
        with ctx.Pool(a.workers) as pool:
            for r in pool.imap_unordered(delta_lote, lotes):
                for k in ("n", "bytes_v2", "patch", "z3_sozinho"):
                    agg[k] += r[k]
                agg["ok"] = agg["ok"] and r["ok"]
        amostra_cli = alterados[:: max(1, len(alterados) // 40)][:40]
        cli = referencia_cli(conj, amostra_cli)
        dif = [abs(x - y) / max(x, 1) for x, y in cli]
        c = saida["conjuntos"][conj]
        arq = c["arquivo_inteiro"]
        v1_z3 = arq["armazenado_z3_com_dedup"] - sum(arq["v2_por_categoria"]["custo_marginal_z3"].values())
        marg = arq["v2_por_categoria"]["custo_marginal_z3"]
        hibrido_v2 = agg["patch"] + marg["novo"]
        saida["delta"][conj] = {
            "rotulo": "limite superior de dedup por similaridade (delta por caminho)",
            "arquivos_alterados": agg["n"],
            "bytes_logicos_alterados_v2": agg["bytes_v2"],
            "patch_from_z3_total": agg["patch"],
            "z3_sozinho_dos_alterados": agg["z3_sozinho"],
            "roundtrip_ok": agg["ok"],
            "armazenado_v1_arquivo_inteiro_z3": v1_z3,
            "custo_marginal_novos_arquivo_inteiro_z3": marg["novo"],
            "armazenado_total_delta_por_caminho": v1_z3 + hibrido_v2,
            "razao_total_delta_por_caminho": (f1.logico + f2.logico) / (v1_z3 + hibrido_v2),
            "armazenado_total_arquivo_inteiro_z3": arq["armazenado_z3_com_dedup"],
            "conferencia_cli_patch_from": {
                "n": len(cli),
                "dif_relativa_media": statistics.mean(dif),
                "dif_relativa_max": max(dif),
                "soma_cli_bytes": sum(x for x, y in cli),
                "soma_api_bytes": sum(y for x, y in cli),
                "nota": "CLI zstd --patch-from=ref -3 --no-check contra compression.zstd com prefixo, window_log ajustado e long-distance matching ligado",
            },
            "por_chunker": {},
            "tempo_s": time.time() - t0,
        }
        for nome, e in c["estrategias"].items():
            if not nome.startswith("cdc_"):
                continue
            m = e["v2_por_categoria"]["custo_marginal_z3"]
            cdc_alt = m["alterado"]
            saida["delta"][conj]["por_chunker"][nome] = {
                "cdc_z3_custo_marginal_alterados": cdc_alt,
                "razao_cdc_sobre_patch_from": cdc_alt / agg["patch"] if agg["patch"] else None,
                "cdc_z3_custo_marginal_novos": m["novo"],
                "armazenado_total_cdc_z3": e["armazenado_z3"],
            }
        log("[delta]", conj, saida["delta"][conj]["arquivos_alterados"], agg["patch"], "%.0fs" % (time.time() - t0))
    salvar_json(a.saida, saida)
    shutil.rmtree(pasta, ignore_errors=True)


def fase_sanidade(a):
    pasta = iniciar()
    rng = np.random.default_rng(SEMENTE_SANIDADE)
    n_a = 256 * MIB
    aleatorio_a = rng.bytes(n_a)
    res = {"semente": SEMENTE_SANIDADE, "media_a": {}, "insercao_b": {}, "roundtrip_c": {}, "determinismo": {}}
    teoria = 1.14
    for nome, cfg in CHUNKERS.items():
        offs, tams = chunkear(aleatorio_a, n_a, cfg)
        media = float(np.mean(tams))
        e = {
            "alvo": cfg[1],
            "n_chunks": int(len(offs)),
            "media": media,
            "media_sobre_alvo": media / cfg[1],
            "min": int(tams.min()),
            "max": int(tams.max()),
            "dentro_dos_limites": True,
        }
        if cfg[0] == "cdc":
            e["dentro_dos_limites"] = bool(int(tams[:-1].min()) >= cfg[1] // 4 and int(tams.max()) <= cfg[1] * 8)
            e["media_teorica_sobre_alvo_paper"] = teoria
            e["aprovado"] = bool(abs(media / cfg[1] - teoria) <= 0.1 and e["dentro_dos_limites"])
            e["aprovado_criterio_25pct_do_alvo"] = bool(abs(media / cfg[1] - 1) <= 0.25)
        else:
            e["aprovado"] = bool(abs(media / cfg[1] - 1) <= 0.001)
        res["media_a"][nome] = e
    del aleatorio_a
    n_b = 64 * MIB
    base = rng.bytes(n_b)
    ins = rng.bytes(100)
    modificado = ins + base
    mvb = memoryview(base)
    mvm = memoryview(modificado)
    for nome, cfg in CHUNKERS.items():
        o1, t1 = chunkear(base, n_b, cfg)
        o2, t2 = chunkear(modificado, len(modificado), cfg)
        h1 = [hash_chunk(mvb[o : o + s]) for o, s in zip(o1.tolist(), t1.tolist())]
        h2 = set(hash_chunk(mvm[o : o + s]) for o, s in zip(o2.tolist(), t2.tolist()))
        reaparecem = sum(1 for h in h1 if h in h2)
        frac = reaparecem / len(h1)
        e = {"chunks_original": len(h1), "reaparecem": reaparecem, "fracao_reaparecem": frac}
        if cfg[0] == "cdc":
            e["aprovado"] = bool(frac >= 0.95)
            e["criterio"] = ">= 95%"
        else:
            e["aprovado"] = bool(frac <= 0.02)
            e["criterio"] = "~0% (<= 2%)"
        res["insercao_b"][nome] = e
        ok_c = True
        for (offs, tams, buf, n) in ((o1, t1, base, n_b), (o2, t2, modificado, len(modificado))):
            contiguo = bool(offs[0] == 0 and np.all(offs[1:] == offs[:-1] + tams[:-1]) and int(offs[-1] + tams[-1]) == n)
            mv = memoryview(buf)
            rec = hashlib.sha256()
            for o, s in zip(offs.tolist(), tams.tolist()):
                rec.update(mv[o : o + s])
            ok_c = ok_c and contiguo and rec.hexdigest() == hashlib.sha256(buf).hexdigest()
            reconstruido = b"".join(mv[o : o + s] for o, s in zip(offs.tolist(), tams.tolist()))
            ok_c = ok_c and reconstruido == buf
        res["roundtrip_c"][nome] = {"aprovado": bool(ok_c)}
        o1b, t1b = chunkear(base, n_b, cfg)
        res["determinismo"][nome] = {"aprovado": bool(np.array_equal(o1, o1b) and np.array_equal(t1, t1b))}
    res["aprovado_tudo"] = all(
        e["aprovado"] for grupo in ("media_a", "insercao_b", "roundtrip_c", "determinismo") for e in res[grupo].values()
    )
    saida = carregar_json(a.saida)
    saida["sanidade"] = res
    salvar_json(a.saida, saida)
    for g in ("media_a", "insercao_b", "roundtrip_c", "determinismo"):
        for n, e in res[g].items():
            log("[sanidade]", g, n, {k: (round(v, 4) if isinstance(v, float) else v) for k, v in e.items()})
    log("sanidade aprovada em tudo:", res["aprovado_tudo"])
    shutil.rmtree(pasta, ignore_errors=True)


def mediana(f, rep):
    ts = []
    for _ in range(rep):
        t0 = time.perf_counter_ns()
        f()
        ts.append((time.perf_counter_ns() - t0) / 1e9)
    return statistics.median(ts), ts


def fase_vazao(a):
    global DADOS
    DADOS = os.path.abspath(a.dados)
    pasta = iniciar()
    cpu = a.cpu
    os.sched_setaffinity(0, {cpu})
    gc.disable()
    fatia = a.fatia_mib * MIB
    f1 = Fonte(DADOS, "tar", "6.12.114")
    f2 = Fonte(DADOS, "tar", "6.12.115")
    b1 = f1.mm
    b2m = f2.mm
    mv1 = memoryview(b1)
    mv2 = memoryview(b2m)
    rep = a.repeticoes
    res = {
        "cpu": cpu,
        "taskset_equivalente": "os.sched_setaffinity(0, {%d})" % cpu,
        "repeticoes": rep,
        "fatia_bytes": fatia,
        "fonte": "primeiros %d MiB do tar de 6.12.115 (lookup preenchido com os primeiros %d MiB do tar de 6.12.114)" % (a.fatia_mib, a.fatia_mib),
        "hash": "hashlib.blake2b(digest_size=32), SUBSTITUTO de BLAKE3 (import blake3 inexistente)",
        "indice_lookup": "dict do Python (NAO representa indice de producao)",
        "chunkers": {},
    }
    mb = fatia / 1e6

    def hash_bruto():
        hashlib.blake2b(mv2[:fatia], digest_size=HASH_BYTES).digest()

    m, ts = mediana(hash_bruto, rep)
    res["blake2b_buffer_unico_MBps"] = mb / m
    for nome, cfg in CHUNKERS.items():
        o1, t1 = chunkear(b1, fatia, cfg)
        indice = {}
        for o, s in zip(o1.tolist(), t1.tolist()):
            indice.setdefault(hash_chunk(mv1[o : o + s]), len(indice))
        o2, t2 = chunkear(b2m, fatia, cfg)
        lo2 = o2.tolist()
        lt2 = t2.tolist()

        def so_chunker():
            chunkear(b2m, fatia, cfg)

        def so_hash():
            b = hashlib.blake2b
            for o, s in zip(lo2, lt2):
                b(mv2[o : o + s], digest_size=HASH_BYTES).digest()

        copias = [dict(indice) for _ in range(rep)]
        estado = {"i": 0, "acertos": 0, "novos": 0}

        def tudo():
            d = copias[estado["i"]]
            estado["i"] += 1
            of, tm = chunkear(b2m, fatia, cfg)
            b = hashlib.blake2b
            acertos = 0
            for o, s in zip(of.tolist(), tm.tolist()):
                h = b(mv2[o : o + s], digest_size=HASH_BYTES).digest()
                if h in d:
                    acertos += 1
                else:
                    d[h] = len(d)
            estado["acertos"] = acertos
            estado["novos"] = len(of) - acertos

        mc, tc = mediana(so_chunker, rep)
        mh, th = mediana(so_hash, rep)
        mt, tt = mediana(tudo, rep)
        res["chunkers"][nome] = {
            "n_chunks": len(lo2),
            "media_chunk_bytes": fatia / len(lo2),
            "chunker_MBps": mb / mc,
            "hash_por_chunk_MBps": mb / mh,
            "chunker_hash_lookup_MBps": mb / mt,
            "tempos_s": {"chunker": tc, "hash": th, "tudo": tt},
            "lookup_acertos_na_ultima_repeticao": estado["acertos"],
            "lookup_novos_na_ultima_repeticao": estado["novos"],
        }
        log("[vazao]", nome, {k: round(v, 1) for k, v in res["chunkers"][nome].items() if k.endswith("MBps")})
    saida = carregar_json(a.saida)
    saida["vazao"] = res
    salvar_json(a.saida, saida)
    shutil.rmtree(pasta, ignore_errors=True)


def fmt_bytes(n):
    for u, d in (("TB", 1e12), ("GB", 1e9), ("MB", 1e6), ("KB", 1e3)):
        if abs(n) >= d:
            return "%.2f %s" % (n / d, u)
    return "%d B" % n


def fmt_pct(x):
    return "%.2f%%" % x


def indice_extrapolado(e, logico_total, n_unicos_chave="n_chunks_unicos"):
    entradas_por_pb = e[n_unicos_chave] / logico_total * PRB_MES
    r = {}
    for rotulo, b in (("so_hash_32B", 32), ("hash_mais_local_40B", 40)):
        tb = entradas_por_pb * b / 1e12
        r[rotulo] = {"TB_por_mes": tb, "meses_em_1TB_ram": 1.0 / tb if tb else None, "meses_em_4TB_ram": 4.0 / tb if tb else None}
    r["chunks_unicos_por_mes"] = entradas_por_pb
    r["pior_caso_sem_dedup_TB_por_mes"] = (e["n_chunks_v1"] + e["n_chunks_v2"]) / logico_total * PRB_MES * 32 / 1e12
    return r


def ordenar(d):
    ordem = list(CHUNKERS)
    return sorted(d.items(), key=lambda kv: ordem.index(kv[0]))


def gate(saida):
    g = {}
    for conj, c in saida["conjuntos"].items():
        L = c["logico_v1"] + c["logico_v2"]
        if c["tipo"] == "arvore":
            base = c["arquivo_inteiro"]["armazenado_z3_com_dedup"]
            base_meta = base + (c["arquivo_inteiro"]["referencias_v1"] + c["arquivo_inteiro"]["referencias_v2"] + c["arquivo_inteiro"]["n_unicos"]) * 32
            rotulo = "arquivo inteiro + zstd-3"
        else:
            base = c["fluxo_sem_dedup_z3"]["armazenado_z3"]
            base_meta = base
            rotulo = "zstd-3 do fluxo inteiro, sem dedup"
        linhas = {}
        for nome, e in ordenar(c["estrategias"]):
            ganho = (base - e["armazenado_z3"]) / base
            meta = (e["referencias_v1"] + e["referencias_v2"] + e["n_chunks_unicos"]) * 32
            ganho_meta = (base_meta - (e["armazenado_z3"] + meta)) / base_meta
            idx = indice_extrapolado(e, L)
            linhas[nome] = {
                "pct_v2_ja_em_v1": e["pct_v2_ja_em_v1"],
                "razao_total_z3": e["razao_z3"],
                "ganho_extra_sobre_baseline": ganho,
                "ganho_extra_com_metadados_32B_por_ref_e_indice": ganho_meta,
                "atende_5pct": bool(ganho >= 0.05),
                "n_chunks_unicos": e["n_chunks_unicos"],
                "indice_teorico_bytes_32B": e["n_chunks_unicos"] * 32,
                "indice_5PB_mes": idx,
                "referencia_z19": e.get("z19"),
            }
        g[conj] = {"baseline": rotulo, "baseline_armazenado_z3": base, "baseline_razao_z3": L / base, "chunkers": linhas}
    return g


def fase_relatorio(a):
    saida = carregar_json(a.saida)
    saida["gate_F1"] = gate(saida)
    salvar_json(a.saida, saida)
    linhas = []
    w = linhas.append
    w("# dedup: F1, dedup por bloco (CDC) contra arquivo inteiro")
    w("")
    w("Gerado por `bench/dedup.py relatorio` a partir de `resultados/dedup.json`. Hash: blake2b-256 no lugar de BLAKE3. Razao = (logico v1 + logico v2) / armazenado. Ganho extra = (armazenado baseline - armazenado CDC) / armazenado baseline, ambos com zstd-3.")
    w("")
    sd = saida.get("sanidade")
    if sd:
        w("## Testes de sanidade (%s)" % ("TODOS APROVADOS" if sd["aprovado_tudo"] else "HA FALHAS"))
        w("")
        w("| chunker | media / alvo (256 MiB aleatorios) | min | max | (a) | reaparecem apos inserir 100 B (64 MiB) | (b) | (c) round-trip | determinismo |")
        w("|---|---|---|---|---|---|---|---|---|")
        for n in CHUNKERS:
            m = sd["media_a"][n]
            i = sd["insercao_b"][n]
            w("| %s | %.3f | %d | %d | %s | %.1f%% (%d/%d) | %s | %s | %s |" % (n, m["media_sobre_alvo"], m["min"], m["max"], "ok" if m["aprovado"] else "FALHA", 100 * i["fracao_reaparecem"], i["reaparecem"], i["chunks_original"], "ok" if i["aprovado"] else "FALHA", "ok" if sd["roundtrip_c"][n]["aprovado"] else "FALHA", "ok" if sd["determinismo"][n]["aprovado"] else "FALHA"))
        w("")
    d = saida.get("dados", {})
    if d.get("versoes_kernel"):
        w("## Dados")
        w("")
        w("| versao | xz (bytes) | sha256 confere | arquivos | vazios | dirs | symlinks | bytes de dados | bytes do tar |")
        w("|---|---|---|---|---|---|---|---|---|")
        for v, i in d["versoes_kernel"].items():
            w("| %s | %d | %s | %d | %d | %d | %d | %d | %d |" % (v, i["bytes_xz"], i["sha256_confere"], i["n_arquivos"], i["n_arquivos_vazios"], i["n_diretorios"], i["n_symlinks"], i["bytes_dados_arvore"], i["bytes_tar"]))
        w("")
        if d.get("sintetico"):
            s = d["sintetico"]
            w("SINTETICO: %s; edicoes in-place %d (%d B, %.2f%%), insercoes %d (%d B), anexo %d B (%.2f%%); backup1 %d B, backup2 %d B." % ("Silesia concatenado em ordem alfabetica", s["edicoes_in_place"]["n"], s["edicoes_in_place"]["bytes"], 100 * s["edicoes_in_place"]["fracao"], s["insercoes"]["n"], s["insercoes"]["bytes"], s["anexo_novo"]["bytes"], 100 * s["anexo_novo"]["fracao"], s["arquivos"]["backup1"]["bytes"], s["arquivos"]["backup2"]["bytes"]))
            w("")
    w("## Gate F1 (zstd-3, limite 5%)")
    w("")
    w("| conjunto | estrategia | % v2 ja em v1 | razao total z3 | ganho extra sobre baseline | atende 5% | ganho c/ metadados | chunks unicos | indice 32 B | indice TB/mes @5PB | meses em 1 TB RAM | meses em 4 TB RAM |")
    w("|---|---|---|---|---|---|---|---|---|---|---|---|")
    for conj, gc_ in saida["gate_F1"].items():
        c = saida["conjuntos"][conj]
        if c["tipo"] == "arvore":
            ai = c["arquivo_inteiro"]
            w("| %s | arquivo inteiro (baseline) | %s | %.3f | 0 (baseline) | | | %d | %s | | | |" % (conj, fmt_pct(ai["pct_v2_ja_em_v1"]), ai["razao_z3_com_dedup"], ai["n_unicos"], fmt_bytes(ai["n_unicos"] * 32)))
        else:
            w("| %s | zstd-3 sem dedup (baseline) | 0 | %.3f | 0 (baseline) | | | | | | | |" % (conj, c["fluxo_sem_dedup_z3"]["razao_z3"]))
        for nome, e in gc_["chunkers"].items():
            ix = e["indice_5PB_mes"]["so_hash_32B"]
            w("| %s | %s | %s | %.3f | %s | %s | %s | %d | %s | %.3f | %.2f | %.2f |" % (conj, nome, fmt_pct(e["pct_v2_ja_em_v1"]), e["razao_total_z3"], fmt_pct(100 * e["ganho_extra_sobre_baseline"]), "sim" if e["atende_5pct"] else "nao", fmt_pct(100 * e["ganho_extra_com_metadados_32B_por_ref_e_indice"]), e["n_chunks_unicos"], fmt_bytes(e["indice_teorico_bytes_32B"]), ix["TB_por_mes"], ix["meses_em_1TB_ram"], ix["meses_em_4TB_ram"]))
    w("")
    w("## Armazenado total (v1+v2), bytes")
    w("")
    w("| conjunto | estrategia | bruto | zstd-3 | zstd-19 (estimado por amostra) | razao bruta | razao z3 | razao z19 | % v2 duplicado (v1 e v2) | media chunk unico | bit-exato |")
    w("|---|---|---|---|---|---|---|---|---|---|---|")
    for conj, c in saida["conjuntos"].items():
        L = c["logico_v1"] + c["logico_v2"]
        if c["tipo"] == "arvore":
            ai = c["arquivo_inteiro"]
            w("| %s | arquivo inteiro sem dedup | %d | %d | | 1.000 | %.3f | | 0 | | %s |" % (conj, ai["armazenado_bruto_sem_dedup"], ai["armazenado_z3_sem_dedup"], ai["razao_z3_sem_dedup"], ai["bit_exato"]))
            w("| %s | arquivo inteiro com dedup | %d | %d | | %.3f | %.3f | | %s | | %s |" % (conj, ai["armazenado_bruto_com_dedup"], ai["armazenado_z3_com_dedup"], ai["razao_bruta_com_dedup"], ai["razao_z3_com_dedup"], fmt_pct(ai["pct_v2_duplicados_total"]), ai["bit_exato"]))
        else:
            fz = c["fluxo_sem_dedup_z3"]
            w("| %s | fluxo sem dedup | %d | %d | | 1.000 | %.3f | | 0 | | %s |" % (conj, L, fz["armazenado_z3"], fz["razao_z3"], fz["bit_exato"]))
            if "fluxo_z3_long31_concatenado" in c:
                lg = c["fluxo_z3_long31_concatenado"]
                w("| %s | EXTRA zstd-3 long=31 (v1+v2 concatenados) | %d | %d | | | %.3f | | | | %s |" % (conj, L, lg["armazenado"], lg["razao"], lg["bit_exato"]))
        for nome, e in ordenar(c["estrategias"]):
            z = e.get("z19")
            w("| %s | %s | %d | %d | %s | %.3f | %.3f | %s | %s | %d | %s |" % (conj, nome, e["armazenado_bruto"], e["armazenado_z3"], ("%d" % z["armazenado_z19_estimado"]) if z and z["armazenado_z19_estimado"] else "", e["razao_bruta"], e["razao_z3"], ("%.3f" % z["razao_z19_estimada"]) if z and z["razao_z19_estimada"] else "", fmt_pct(e["pct_v2_duplicados_total"]), e["media_chunk_unico"], e["bit_exato"]))
    w("")
    if saida.get("delta"):
        w("## Delta por caminho (limite superior de dedup por similaridade)")
        w("")
        w("| conjunto | arquivos alterados | bytes logicos alterados (v2) | zstd-3 sozinho | patch-from z3 | CDC 8K marginal | CDC 64K marginal | CDC 1M marginal | total arquivo inteiro z3 | total delta por caminho | total CDC 8K z3 | total CDC 64K z3 | razao delta | razao CDC 64K |")
        w("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
        for conj, dd in saida["delta"].items():
            pc = dd["por_chunker"]
            c = saida["conjuntos"][conj]
            L = c["logico_v1"] + c["logico_v2"]
            w("| %s | %d | %d | %d | %d | %d | %d | %d | %d | %d | %d | %d | %.3f | %.3f |" % (conj, dd["arquivos_alterados"], dd["bytes_logicos_alterados_v2"], dd["z3_sozinho_dos_alterados"], dd["patch_from_z3_total"], pc["cdc_8K"]["cdc_z3_custo_marginal_alterados"], pc["cdc_64K"]["cdc_z3_custo_marginal_alterados"], pc["cdc_1M"]["cdc_z3_custo_marginal_alterados"], dd["armazenado_total_arquivo_inteiro_z3"], dd["armazenado_total_delta_por_caminho"], pc["cdc_8K"]["armazenado_total_cdc_z3"], pc["cdc_64K"]["armazenado_total_cdc_z3"], dd["razao_total_delta_por_caminho"], L / pc["cdc_64K"]["armazenado_total_cdc_z3"]))
        w("")
        w("Conferencia do prefixo de `compression.zstd` contra `zstd --patch-from` (CLI): " + "; ".join("%s: %d arquivos, diferenca relativa media %.3f%%, max %.3f%%" % (k, v["conferencia_cli_patch_from"]["n"], 100 * v["conferencia_cli_patch_from"]["dif_relativa_media"], 100 * v["conferencia_cli_patch_from"]["dif_relativa_max"]) for k, v in saida["delta"].items()))
        w("")
    vz = saida.get("vazao")
    if vz:
        w("## Vazao em 1 nucleo (CPU %d, mediana de %d, MB = 1e6 B)" % (vz["cpu"], vz["repeticoes"]))
        w("")
        w("Fonte: %s. Hash: %s. Lookup: %s. blake2b em buffer unico: %.0f MB/s." % (vz["fonte"], vz["hash"], vz["indice_lookup"], vz["blake2b_buffer_unico_MBps"]))
        w("")
        w("| chunker | chunks | media | chunker MB/s | hash por chunk MB/s | chunker+hash+lookup MB/s |")
        w("|---|---|---|---|---|---|")
        for n, e in ordenar(vz["chunkers"]):
            w("| %s | %d | %d | %s | %.0f | %.0f |" % (n, e["n_chunks"], e["media_chunk_bytes"], ("%.0f" % e["chunker_MBps"]) if n.startswith("cdc_") else "n/a (aritmetica de offsets)", e["hash_por_chunk_MBps"], e["chunker_hash_lookup_MBps"]))
        w("")
    viol = saida.get("bit_exatidao_violada")
    w("## Bit-exatidao")
    w("")
    w("Violacoes: %s." % ("nenhuma (reconstrucao de cada arquivo da arvore e de cada fluxo a partir de chunks unicos + receitas, sha256 igual ao original; round-trip zstd-3 de cada chunk unico)" if viol == [] else json.dumps(viol)))
    w("")
    with open(a.md, "w") as f:
        f.write("\n".join(linhas) + "\n")
    log("relatorio escrito em", a.md)


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("preparar")
    p.add_argument("--dados", required=True)
    p.add_argument("--silesia", default="")
    p.add_argument("--versoes", default=",".join(VERSOES_KERNEL))
    p.add_argument("--saida", required=True)
    p = sub.add_parser("sanidade")
    p.add_argument("--saida", required=True)
    p = sub.add_parser("analise")
    p.add_argument("--dados", required=True)
    p.add_argument("--saida", required=True)
    p.add_argument("--workers", type=int, default=16)
    p.add_argument("--amostra19", type=int, default=4)
    p.add_argument("--conjuntos", default="")
    p = sub.add_parser("long")
    p.add_argument("--dados", required=True)
    p.add_argument("--saida", required=True)
    p = sub.add_parser("delta")
    p.add_argument("--dados", required=True)
    p.add_argument("--saida", required=True)
    p.add_argument("--workers", type=int, default=16)
    p = sub.add_parser("vazao")
    p.add_argument("--dados", required=True)
    p.add_argument("--saida", required=True)
    p.add_argument("--cpu", type=int, default=0)
    p.add_argument("--repeticoes", type=int, default=3)
    p.add_argument("--fatia-mib", type=int, default=256)
    p = sub.add_parser("relatorio")
    p.add_argument("--saida", required=True)
    p.add_argument("--md", required=True)
    a = ap.parse_args()
    {
        "preparar": fase_preparar,
        "sanidade": fase_sanidade,
        "analise": fase_analise,
        "long": fase_long,
        "delta": fase_delta,
        "vazao": fase_vazao,
        "relatorio": fase_relatorio,
    }[a.cmd](a)


if __name__ == "__main__":
    main()
