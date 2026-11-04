import argparse
import bz2
import gc
import hashlib
import importlib.util
import json
import lzma
import math
import multiprocessing as mp
import os
import platform
import statistics
import sys
import time
import urllib.request

import numpy as np
from compression import zstd

KIB = 1 << 10
MIB = 1 << 20
BASE = os.path.dirname(os.path.abspath(__file__))
GRANS = [("64K", 64 * KIB), ("256K", 256 * KIB), ("1M", MIB)]
LIMITE_KERNEL = 256 * MIB
URL_KERNEL = "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.12.115.tar.xz"
TRANSFORMS = [("d1", "delta", 1), ("d2", "delta", 2), ("d4", "delta", 4), ("d8", "delta", 8), ("b4", "bsplit", 4), ("b8", "bsplit", 8)]
SYNC = ["stored", "zstd-1", "zstd-3"] + [t[0] + "+zstd-3" for t in TRANSFORMS]
ASYNC = ["zstd-19", "xz-9e", "bzip2-9", "T+zstd-19", "T+xz-9e"]
CANDS = SYNC + ASYNC
NS = len(SYNC)
NC = len(CANDS)
CPUS_ROTULO = [0, 2, 4, 6, 8, 10]
CPU_TEMPO = 0
ORDEM_CLASSES = ["texto", "codigo", "codigo_kernel", "binario", "cientifico", "banco_de_dados", "xml", "dna", "sint_logs", "sint_incompressivel", "sint_ja_comprimido"]
CLASSE_VAZAO = {"codigo_kernel": "codigo"}
CENARIOS = [("nuvem", "s3_standard"), ("onprem", "onprem")]
CAMADAS = ["quente", "morno", "frio"]
ROTULOS_ECO = ["%s+%s|%s" % (c, a, t) for c, a in CENARIOS for t in CAMADAS]
LIMIAR_PROBE = 0.98
H0_STORED = 7.9
MARGEM_REGRA = 0.5
VAZAO_MIN_SINCRONO = 290.0


def carregar_modulo(nome, arquivo):
    spec = importlib.util.spec_from_file_location(nome, arquivo)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


SB = carregar_modulo("syntrabench", os.path.join(BASE, "syntrabench.py"))
ECO = carregar_modulo("economia", os.path.join(BASE, "economia.py"))


def delta_apply(b, w):
    n = len(b) // w
    if n == 0:
        return bytes(b)
    dt = np.dtype("<u%d" % w)
    a = np.frombuffer(b, dtype=dt, count=n)
    d = np.empty_like(a)
    d[0] = a[0]
    np.subtract(a[1:], a[:-1], out=d[1:])
    return d.tobytes() + bytes(b[n * w :])


def delta_invert(b, w):
    n = len(b) // w
    if n == 0:
        return bytes(b)
    dt = np.dtype("<u%d" % w)
    a = np.frombuffer(b, dtype=dt, count=n)
    return np.cumsum(a, dtype=dt).tobytes() + bytes(b[n * w :])


def bsplit_apply(b, w):
    n = len(b) // w
    if n == 0:
        return bytes(b)
    a = np.frombuffer(b, dtype=np.uint8, count=n * w).reshape(n, w)
    return np.ascontiguousarray(a.T).tobytes() + bytes(b[n * w :])


def bsplit_invert(b, w):
    n = len(b) // w
    if n == 0:
        return bytes(b)
    p = np.frombuffer(b, dtype=np.uint8, count=n * w).reshape(w, n)
    return np.ascontiguousarray(p.T).tobytes() + bytes(b[n * w :])


def aplicar(b, tipo, w):
    return delta_apply(b, w) if tipo == "delta" else bsplit_apply(b, w)


def inverter(b, tipo, w):
    return delta_invert(b, w) if tipo == "delta" else bsplit_invert(b, w)


def delta_ref(b, w):
    n = len(b) // w
    out = bytearray()
    prev = 0
    for i in range(n):
        cur = int.from_bytes(b[i * w : (i + 1) * w], "little")
        out += ((cur - prev) % (1 << (8 * w))).to_bytes(w, "little")
        prev = cur
    return bytes(out) + bytes(b[n * w :])


def bsplit_ref(b, w):
    n = len(b) // w
    out = bytearray(len(b))
    for p in range(w):
        for i in range(n):
            out[p * n + i] = b[i * w + p]
    out[n * w :] = b[n * w :]
    return bytes(out)


def autoteste_transforms():
    rng = np.random.default_rng(7)
    falhas = []
    casos = 0
    for tam in list(range(0, 41)) + [255, 256, 257, 1000, 1023, 4097]:
        for modo in ("aleatorio", "constante", "rampa"):
            if modo == "aleatorio":
                b = rng.integers(0, 256, tam, dtype=np.uint8).tobytes()
            elif modo == "constante":
                b = bytes([255]) * tam
            else:
                b = bytes((i * 37) % 256 for i in range(tam))
            for nome, tipo, w in TRANSFORMS:
                y = aplicar(b, tipo, w)
                ref = delta_ref(b, w) if tipo == "delta" else bsplit_ref(b, w)
                casos += 1
                if y != ref or inverter(y, tipo, w) != b or len(y) != len(b):
                    falhas.append((nome, tam, modo))
    return {"casos": casos, "falhas": falhas[:10], "n_falhas": len(falhas), "passou": not falhas}


def listar_corpus(dados, kernel):
    with open(os.path.join(dados, "manifesto.json")) as f:
        man = json.load(f)
    arqs = []
    for a in man["arquivos"]:
        arqs.append({"nome": a["nome"], "classe": a["classe"], "caminho": os.path.join(dados, a["caminho"]), "sintetico": a.get("sintetico", False)})
    if kernel and os.path.isdir(kernel):
        for nome in sorted(os.listdir(kernel)):
            if nome.endswith(".bin"):
                arqs.append({"nome": "kernel/" + nome[:-4], "classe": "codigo_kernel", "caminho": os.path.join(kernel, nome), "sintetico": False})
    for a in arqs:
        a["tamanho"] = os.path.getsize(a["caminho"])
    return arqs


class Fluxo:
    def __init__(self, url):
        req = urllib.request.Request(url, headers={"User-Agent": "plano_bloco/1.0"})
        self.r = urllib.request.urlopen(req, timeout=120)
        self.d = lzma.LZMADecompressor(format=lzma.FORMAT_XZ)
        self.buf = bytearray()
        self.i = 0
        self.pos = 0
        self.baixados = 0
        self.fim = False

    def encher(self, n):
        while len(self.buf) - self.i < n and not self.fim:
            c = self.r.read(1 << 20)
            if not c:
                self.fim = True
                break
            self.baixados += len(c)
            if self.d.eof:
                self.fim = True
                break
            self.buf += self.d.decompress(c)
            if self.d.eof:
                self.fim = True

    def ler(self, n):
        self.encher(n)
        out = bytes(self.buf[self.i : self.i + n])
        self.i += len(out)
        self.pos += len(out)
        if self.i > (4 << 20):
            del self.buf[: self.i]
            self.i = 0
        return out


def tamanho_tar(campo):
    if campo[0] & 0x80:
        return int.from_bytes(bytes([campo[0] & 0x7F]) + campo[1:], "big")
    t = campo.split(b"\0")[0].strip()
    return int(t, 8) if t else 0


def registros_pax(dados):
    saida = {}
    i = 0
    while i < len(dados):
        j = dados.index(b" ", i)
        n = int(dados[i:j])
        reg = dados[j + 1 : i + n - 1]
        k, _, v = reg.partition(b"=")
        saida[k.decode("utf-8", "replace")] = v
        i += n
    return saida


def grupo_de(nome):
    partes = nome.decode("utf-8", "replace").strip("/").split("/")[1:]
    if len(partes) <= 1:
        return ("raiz", "_")
    return (partes[0], partes[1] if len(partes) > 2 else "_")


def seguro(s):
    return "".join(c if c.isalnum() or c in "-_" else "_" for c in s)


def extrair_kernel(url, destino, limite):
    os.makedirs(destino, exist_ok=True)
    if os.listdir(destino):
        sys.exit("diretorio precisa estar vazio: " + destino)
    fl = Fluxo(url)
    grupos = {}
    contagem = {}
    nome_longo = None
    pax = None
    pax_global = None
    n_reg = 0
    while fl.pos < limite:
        cab = fl.ler(512)
        if len(cab) < 512 or cab.count(0) == 512:
            break
        nome = cab[:100].split(b"\0")[0]
        if cab[257:262] == b"ustar":
            pre = cab[345:500].split(b"\0")[0]
            if pre:
                nome = pre + b"/" + nome
        tipo = cab[156:157]
        tam = tamanho_tar(cab[124:136])
        pad = (-tam) % 512
        if tipo in (b"x", b"g", b"L", b"K"):
            dados = fl.ler(tam)
            fl.ler(pad)
            if tipo == b"L":
                nome_longo = dados.split(b"\0")[0]
            elif tipo == b"x":
                pax = registros_pax(dados)
            elif tipo == b"g":
                pax_global = registros_pax(dados)
            continue
        if nome_longo is not None:
            nome = nome_longo
        if pax and "path" in pax:
            nome = pax["path"]
        if pax and "size" in pax:
            tam = int(pax["size"])
            pad = (-tam) % 512
        nome_longo = None
        pax = None
        if tipo in (b"0", b"\0", b"7"):
            chave = grupo_de(nome)
            lista = grupos.setdefault(chave, [])
            restante = tam
            while restante > 0:
                n = min(restante, 1 << 20, limite - fl.pos)
                if n <= 0:
                    break
                p = fl.ler(n)
                if not p:
                    break
                lista.append(p)
                restante -= len(p)
            contagem[chave] = contagem.get(chave, 0) + 1
            n_reg += 1
            if restante > 0:
                break
            fl.ler(pad)
        else:
            fl.ler(tam + pad)
    tops = {}
    for (top, sub), lista in grupos.items():
        tops.setdefault(top, {})[sub] = (sum(len(p) for p in lista), lista, contagem[(top, sub)])
    finais = {}
    for top, subs in sorted(tops.items()):
        total = sum(v[0] for v in subs.values())
        if total <= 40 * MIB:
            finais[top] = [x for s in sorted(subs) for x in subs[s][1]], sum(v[2] for v in subs.values())
        else:
            resto = []
            nresto = 0
            for s in sorted(subs):
                if subs[s][0] >= 8 * MIB:
                    finais[top + "_" + s] = subs[s][1], subs[s][2]
                else:
                    resto += subs[s][1]
                    nresto += subs[s][2]
            if resto:
                finais[top + "__resto"] = resto, nresto
    manifesto = {"url": URL_KERNEL, "bytes_tar_consumidos": fl.pos, "bytes_xz_baixados": fl.baixados, "limite_bytes": limite, "arquivos_regulares_lidos": n_reg, "grupos": {}}
    for nome, (lista, nf) in sorted(finais.items()):
        caminho = os.path.join(destino, "kernel_" + seguro(nome) + ".bin")
        h = hashlib.sha256()
        total = 0
        with open(caminho, "wb") as f:
            for p in lista:
                f.write(p)
                h.update(p)
                total += len(p)
        manifesto["grupos"][os.path.basename(caminho)] = {"bytes": total, "arquivos_tar": nf, "sha256": h.hexdigest()}
    with open(os.path.join(destino, "..", "kernel_manifesto.json"), "w") as f:
        json.dump(manifesto, f, indent=1)
    return manifesto


def cmd_kernel(a):
    m = extrair_kernel(a.url, a.destino, a.limite_mib * MIB)
    tot = sum(g["bytes"] for g in m["grupos"].values())
    print("grupos:", len(m["grupos"]), "bytes de conteudo:", tot, "bytes tar consumidos:", m["bytes_tar_consumidos"], "xz baixado:", m["bytes_xz_baixados"])
    for n, g in m["grupos"].items():
        print(" ", n, g["bytes"], g["arquivos_tar"])


def fabricar_codecs(gran):
    c = {}
    for nome in ("zstd-1", "zstd-3", "zstd-19", "xz-9e", "bzip2-9"):
        c[nome] = SB.fabricar(nome, gran)
    return c


def sha(b):
    return hashlib.sha256(b).digest()


def rotular_bloco(x, cod, limiar):
    L = len(x)
    h = sha(x)
    tam = np.full(NC, -1, np.int32)
    ok = np.zeros(NC, np.bool_)
    falhas = []
    tam[0] = L
    ok[0] = sha(bytes(x)) == h
    for j, nome in ((1, "zstd-1"), (2, "zstd-3")):
        c = cod[nome][0](x)
        tam[j] = len(c)
        ok[j] = sha(cod[nome][1](c)) == h
    z3c, z3d = cod["zstd-3"]
    transformados = []
    for ti, (nome, tipo, w) in enumerate(TRANSFORMS):
        y = aplicar(x, tipo, w)
        rev = inverter(y, tipo, w) == x
        cy = z3c(y)
        r2 = inverter(z3d(cy), tipo, w)
        tam[3 + ti] = len(cy)
        ok[3 + ti] = rev and sha(r2) == h
        if not rev:
            falhas.append((nome, "transform nao reversivel"))
        transformados.append(y)
    mi = int(np.argmin(tam[3:NS]))
    ref = min(int(tam[2]), L)
    ts = mi if tam[3 + mi] < (1.0 - limiar) * ref else -1
    for j, nome in ((NS, "zstd-19"), (NS + 1, "xz-9e"), (NS + 2, "bzip2-9")):
        c = cod[nome][0](x)
        tam[j] = len(c)
        ok[j] = sha(cod[nome][1](c)) == h
    if ts >= 0:
        _, tipo, w = TRANSFORMS[ts]
        y = transformados[ts]
        for j, nome in ((NS + 3, "zstd-19"), (NS + 4, "xz-9e")):
            c = cod[nome][0](y)
            tam[j] = len(c)
            ok[j] = sha(inverter(cod[nome][1](c), tipo, w)) == h
    for j in range(NC):
        if tam[j] >= 0 and not ok[j]:
            falhas.append((CANDS[j], "sha256 divergente"))
    return tam, ok, ts, falhas


def tarefa_rotular(t):
    g, fi, caminho, i0, i1, gran, limiar = t
    t0 = time.perf_counter()
    with open(caminho, "rb") as f:
        f.seek(i0 * gran)
        buf = f.read((i1 - i0) * gran)
    cod = fabricar_codecs(gran)
    n = i1 - i0
    tam = np.zeros((n, NC), np.int32)
    ok = np.zeros((n, NC), np.bool_)
    ts = np.zeros(n, np.int8)
    ln = np.zeros(n, np.int32)
    falhas = []
    for k in range(n):
        x = buf[k * gran : (k + 1) * gran]
        ln[k] = len(x)
        tam[k], ok[k], ts[k], fl = rotular_bloco(x, cod, limiar)
        for nome, motivo in fl:
            falhas.append({"arquivo_idx": fi, "bloco": i0 + k, "candidato": nome, "motivo": motivo})
    return (g, fi, i0), {"tam": tam, "ok": ok, "ts": ts, "len": ln, "falhas": falhas, "seg": time.perf_counter() - t0}


_contador = None


def iniciar_trabalhador(contador, cpus):
    with contador.get_lock():
        i = contador.value
        contador.value += 1
    os.sched_setaffinity(0, {cpus[i % len(cpus)]})
    gc.disable()


def montar_tarefas(corpus, limiar, passo_mib=8):
    tarefas = []
    for fi, arq in enumerate(corpus):
        for g, gran in GRANS:
            nb = -(-arq["tamanho"] // gran)
            passo = max(1, (passo_mib * MIB) // gran)
            for i0 in range(0, nb, passo):
                tarefas.append((g, fi, arq["caminho"], i0, min(nb, i0 + passo), gran, limiar))
    return tarefas


def cmd_rotular(a):
    corpus = listar_corpus(a.dados, a.kernel)
    tarefas = montar_tarefas(corpus, a.limiar_transform)
    tarefas.sort(key=lambda t: -(t[4] - t[3]) * t[5])
    nproc = min(a.processos, 6)
    ctx = mp.get_context("fork")
    cont = ctx.Value("i", 0)
    t0 = time.perf_counter()
    res = {}
    with ctx.Pool(nproc, initializer=iniciar_trabalhador, initargs=(cont, CPUS_ROTULO)) as pool:
        for k, (chave, r) in enumerate(pool.imap_unordered(tarefa_rotular, tarefas, chunksize=1)):
            res[chave] = r
            if k % 20 == 0:
                print("tarefas %d/%d  %.0fs" % (k + 1, len(tarefas), time.perf_counter() - t0), flush=True)
    saida = {}
    falhas = []
    for g, gran in GRANS:
        chaves = sorted(c for c in res if c[0] == g)
        arq, off, ln, tam, ok, ts = [], [], [], [], [], []
        for (_, fi, i0) in chaves:
            r = res[(g, fi, i0)]
            n = len(r["len"])
            arq.append(np.full(n, fi, np.int16))
            off.append((i0 + np.arange(n)) * gran)
            ln.append(r["len"])
            tam.append(r["tam"])
            ok.append(r["ok"])
            ts.append(r["ts"])
            falhas += r["falhas"]
        saida[g + "_arq"] = np.concatenate(arq)
        saida[g + "_off"] = np.concatenate(off)
        saida[g + "_len"] = np.concatenate(ln)
        saida[g + "_tam"] = np.concatenate(tam)
        saida[g + "_ok"] = np.concatenate(ok)
        saida[g + "_ts"] = np.concatenate(ts)
    saida["nomes"] = np.array([x["nome"] for x in corpus])
    saida["classes"] = np.array([x["classe"] for x in corpus])
    saida["cands"] = np.array(CANDS)
    np.savez_compressed(a.saida, **saida)
    n_nao_exato = {g: int(((saida[g + "_tam"] >= 0) & ~saida[g + "_ok"]).sum()) for g, _ in GRANS}
    meta = {
        "falhas": falhas,
        "nao_exatos": n_nao_exato,
        "tempo_s": time.perf_counter() - t0,
        "limiar_transform": a.limiar_transform,
        "processos": nproc,
        "cpus": CPUS_ROTULO[:nproc],
        "blocos": {g: int(len(saida[g + "_len"])) for g, _ in GRANS},
        "comparacoes_bit_exatas": {g: int((saida[g + "_tam"] >= 0).sum()) for g, _ in GRANS},
        "corpus": corpus,
    }
    with open(a.saida + ".meta.json", "w") as f:
        json.dump(meta, f, indent=1)
    print("blocos:", meta["blocos"], "comparacoes:", meta["comparacoes_bit_exatas"], "falhas:", len(falhas), "nao exatos:", n_nao_exato, "tempo: %.0fs" % meta["tempo_s"])


FEATS = ["h0", "h1", "nd", "fz", "fp", "fmax", "run", "hd1", "hd2", "hd4", "hd8", "hp4", "hp8", "ac2", "ac4", "ac8", "cr2", "cr4", "cr8", "pr", "lenfrac"]
NF = len(FEATS)
FIDX = {n: i for i, n in enumerate(FEATS)}
VARIANTES = [("inicio", 4096), ("espalhado", 4096), ("inicio", 16384), ("espalhado", 16384)]
POS = {w: (np.arange(16384, dtype=np.int32) % w) * 256 for w in (4, 8)}
NATIVO_MBPS = {"delta": 4000.0, "bsplit": 3000.0}
SUBCONJ_REGRA = ["h0", "hd1", "hd2", "hd4", "hd8", "hp4", "hp8"]


def ent_cnt(c):
    c = c[c > 0]
    p = c / c.sum()
    return float(-(p * np.log2(p)).sum())


def amostrar(bl, politica, S):
    L = len(bl)
    if L <= S:
        return bl
    if politica == "inicio":
        return bl[:S]
    q = S // 4
    offs = [((L - q) * i // 3) // 8 * 8 for i in range(4)]
    return np.concatenate([bl[o : o + q] for o in offs])


class Amostra:
    def __init__(self, bl, politica, S, gran):
        self.bl = bl
        self.politica = politica
        self.gran = gran
        self.x = amostrar(bl, politica, S)
        self.n = len(self.x)
        self.memo = {}

    def valor(self, nome):
        if nome not in self.memo:
            self.memo[nome] = getattr(self, "f_" + nome)()
        return self.memo[nome]

    def cnt(self):
        if "_cnt" not in self.memo:
            self.memo["_cnt"] = np.bincount(self.x, minlength=256)
        return self.memo["_cnt"]

    def f_h0(self):
        return ent_cnt(self.cnt())

    def f_nd(self):
        return float(np.count_nonzero(self.cnt())) / 256.0

    def f_fz(self):
        return float(self.cnt()[0]) / self.n

    def f_fp(self):
        c = self.cnt()
        return float(c[32:127].sum() + c[9] + c[10] + c[13]) / self.n

    def f_fmax(self):
        return float(self.cnt().max()) / self.n

    def f_run(self):
        if self.n < 2:
            return 0.0
        runs = 1 + int(np.count_nonzero(self.x[1:] != self.x[:-1]))
        return math.log2(self.n / runs)

    def f_h1(self):
        if self.n < 2:
            return 0.0
        idx = (self.x[:-1] >> 3).astype(np.int32) * 256 + self.x[1:]
        c2 = np.bincount(idx, minlength=8192)
        return ent_cnt(c2) - ent_cnt(c2.reshape(32, 256).sum(1))

    def f_hd1(self):
        if self.n < 2:
            return self.valor("h0")
        return ent_cnt(np.bincount(self.x[1:] - self.x[:-1], minlength=256))

    def hdw(self, w):
        m = (self.n // w) * w
        if m < 2 * w:
            return self.valor("h0")
        v = self.x[:m].view("<u%d" % w)
        d = v[1:] - v[:-1]
        return ent_cnt(np.bincount(d.view(np.uint8), minlength=256))

    def f_hd2(self):
        return self.hdw(2)

    def f_hd4(self):
        return self.hdw(4)

    def f_hd8(self):
        return self.hdw(8)

    def hpw(self, w):
        m = (self.n // w) * w
        if m < 2 * w:
            return self.valor("h0")
        idx = POS[w][:m] + self.x[:m]
        return ent_cnt(np.bincount(idx, minlength=256 * w)) - math.log2(w)

    def f_hp4(self):
        return self.hpw(4)

    def f_hp8(self):
        return self.hpw(8)

    def ac(self, l):
        if self.n <= l:
            return 0.0
        return float(np.count_nonzero(self.x[l:] == self.x[:-l])) / (self.n - l)

    def f_ac2(self):
        return self.ac(2)

    def f_ac4(self):
        return self.ac(4)

    def f_ac8(self):
        return self.ac(8)

    def xc(self):
        if "_xc" not in self.memo:
            xf = self.x.astype(np.float32)
            xf -= xf.mean()
            self.memo["_xc"] = (xf, float(np.dot(xf, xf)))
        return self.memo["_xc"]

    def cr(self, l):
        if self.n <= l:
            return 0.0
        xf, den = self.xc()
        if den < 1e-9:
            return 0.0
        return float(np.dot(xf[l:], xf[:-l])) / den

    def f_cr2(self):
        return self.cr(2)

    def f_cr4(self):
        return self.cr(4)

    def f_cr8(self):
        return self.cr(8)

    def f_pr(self):
        p = amostrar(self.bl, self.politica, 4096)
        return len(zstd.compress(p, level=1)) / len(p)

    def f_lenfrac(self):
        return len(self.bl) / self.gran


def extrair(bl, politica, S, gran, nomes):
    a = Amostra(bl, politica, S, gran)
    return [a.valor(n) for n in nomes]


def tarefa_features(t):
    g, fi, caminho, i0, i1, gran = t
    with open(caminho, "rb") as f:
        f.seek(i0 * gran)
        buf = f.read((i1 - i0) * gran)
    n = i1 - i0
    X = np.zeros((n, len(VARIANTES), NF), np.float32)
    for k in range(n):
        bl = np.frombuffer(buf, dtype=np.uint8, count=min(gran, len(buf) - k * gran), offset=k * gran)
        for vi, (pol, S) in enumerate(VARIANTES):
            X[k, vi] = extrair(bl, pol, S, gran, FEATS)
    return (g, fi, i0), X


def cmd_features(a):
    corpus = listar_corpus(a.dados, a.kernel)
    tarefas = [t[:6] for t in montar_tarefas(corpus, 0.0)]
    tarefas.sort(key=lambda t: -(t[4] - t[3]) * t[5])
    ctx = mp.get_context("fork")
    cont = ctx.Value("i", 0)
    res = {}
    t0 = time.perf_counter()
    with ctx.Pool(min(a.processos, 6), initializer=iniciar_trabalhador, initargs=(cont, CPUS_ROTULO)) as pool:
        for chave, X in pool.imap_unordered(tarefa_features, tarefas, chunksize=1):
            res[chave] = X
    saida = {}
    for g, gran in GRANS:
        chaves = sorted(c for c in res if c[0] == g)
        saida[g + "_X"] = np.concatenate([res[c] for c in chaves])
    saida["feats"] = np.array(FEATS)
    np.savez_compressed(a.saida, **saida)
    print("features:", {g: saida[g + "_X"].shape for g, _ in GRANS}, "tempo: %.0fs" % (time.perf_counter() - t0))


def mediana_ns(fn, reps=3):
    ts = []
    for _ in range(reps):
        t0 = time.perf_counter_ns()
        fn()
        ts.append(time.perf_counter_ns() - t0)
    return statistics.median(ts)


def cmd_custo_transform(a):
    os.sched_setaffinity(0, {CPU_TEMPO})
    gc.disable()
    rng = np.random.default_rng(3)
    buf = rng.integers(0, 256, MIB, dtype=np.uint8).tobytes()
    saida = {}
    for nome, tipo, w in TRANSFORMS:
        for _ in range(5):
            y = aplicar(buf, tipo, w)
            inverter(y, tipo, w)
        ta = [mediana_ns(lambda: aplicar(buf, tipo, w), 1) for _ in range(31)]
        ti = [mediana_ns(lambda: inverter(y, tipo, w), 1) for _ in range(31)]
        saida[nome] = {"apply_MBps": MIB / (statistics.median(ta) * 1e-9) / 1e6, "invert_MBps": MIB / (statistics.median(ti) * 1e-9) / 1e6, "nativo_estimado_MBps": NATIVO_MBPS[tipo]}
    saida["_meta"] = {"numpy": np.__version__, "cpu": CPU_TEMPO, "buffer_bytes": MIB, "repeticoes": 31, "estatistica": "mediana"}
    with open(a.saida, "w") as f:
        json.dump(saida, f, indent=1)
    for k, v in saida.items():
        if k != "_meta":
            print(k, "apply %.0f MB/s  invert %.0f MB/s" % (v["apply_MBps"], v["invert_MBps"]))


def serie(a, b):
    return 1.0 / (1.0 / a + 1.0 / b)


def cod_do_candidato(j):
    if j < 3:
        return ["stored", "zstd-1", "zstd-3"][j], None
    if j < NS:
        return "zstd-3", j - 3
    if j == NS:
        return "zstd-19", None
    if j == NS + 1:
        return "xz-9e", None
    if j == NS + 2:
        return "bzip2-9", None
    if j == NS + 3:
        return "zstd-19", -1
    return "xz-9e", -1


def preparar_economia(medicoes, precos, transformes, modo, classes):
    med = ECO.carregar(medicoes)
    p = ECO.carregar(precos)
    ctx = ECO.Contexto(p, med)
    th = {}
    for r in ECO.agregar(med):
        if r["modo"] == "bloco_1MiB":
            th[(r["classe"], r["config"])] = (r["theta_c_MBps"], r["theta_d_MBps"])
    with open(transformes) as f:
        tr = json.load(f)
    chave_tr = "apply_MBps" if modo == "numpy" else "nativo_estimado_MBps"
    chave_ti = "invert_MBps" if modo == "numpy" else "nativo_estimado_MBps"
    ncl = len(classes)
    THC = np.zeros((ncl, NC, 7))
    THD = np.zeros((ncl, NC, 7))
    CC = {c: np.zeros((ncl, NC, 7)) for c, _ in CENARIOS}
    CD = {c: np.zeros((ncl, NC, 7)) for c, _ in CENARIOS}
    for ci, cl in enumerate(classes):
        base = CLASSE_VAZAO.get(cl, cl)
        for j in range(NC):
            codec, ti_fixo = cod_do_candidato(j)
            tc0, td0 = th[(base, codec)]
            for k in range(7):
                if ti_fixo is None:
                    tc, td = tc0, td0
                else:
                    ti = ti_fixo if ti_fixo >= 0 else k - 1
                    if ti < 0:
                        tc, td = tc0, td0
                    else:
                        nome = TRANSFORMS[ti][0]
                        tc = serie(tc0, tr[nome][chave_tr])
                        td = serie(td0, tr[nome][chave_ti])
                THC[ci, j, k] = tc
                THD[ci, j, k] = td
                perf = ctx.perfil(tc, td, 1.0, j == 0)
                for c, _ in CENARIOS:
                    CC[c][ci, j, k] = perf[c]["custo_comp_usd_por_gb"]
                    CD[c][ci, j, k] = perf[c]["custo_decomp_usd_por_gb"]
    return {"ctx": ctx, "th": th, "tr": tr, "THC": THC, "THD": THD, "CC": CC, "CD": CD, "med": med, "p": p, "modo": modo}


def construir_valores(d, E):
    ctx = E["ctx"]
    n = len(d["len"])
    L = d["len"].astype(np.float64)[:, None]
    tam = d["tam"]
    valido = tam >= 0
    eff = np.where(valido, np.minimum(tam, d["len"][:, None]), np.inf).astype(np.float64)
    s = np.where(valido, np.clip(1.0 - eff / L, 0.0, 1.0), 0.0)
    kk = np.zeros((n, NC), np.int64)
    kk[:, NS + 3] = np.maximum(d["ts"].astype(np.int64) + 1, 0)
    kk[:, NS + 4] = kk[:, NS + 3]
    jj = np.broadcast_to(np.arange(NC)[None, :], (n, NC))
    cl = np.broadcast_to(d["cls"][:, None], (n, NC))
    d["tam_eff"] = eff
    d["s"] = s
    d["tc"] = L / (E["THC"][cl, jj, kk] * 1e6)
    d["td"] = L / (E["THD"][cl, jj, kk] * 1e6)
    d["cc"] = {}
    V = {}
    for c, arm in CENARIOS:
        cc = E["CC"][c][cl, jj, kk]
        cd = E["CD"][c][cl, jj, kk]
        d["cc"][c] = cc
        valor_arm = s * ctx.preco_arm[arm] * ctx.T * ctx.red
        for t in CAMADAS:
            v = valor_arm - cc - ctx.n_leituras[t] * np.where(s > 0, cd, 0.0)
            v[~valido] = -np.inf
            V["%s+%s|%s" % (c, arm, t)] = v
    d["V"] = V


def verificar_valores_contra_economia(d, E, n=200):
    rng = np.random.default_rng(11)
    ctx = E["ctx"]
    pior = 0.0
    nb = len(d["len"])
    for _ in range(n):
        b = int(rng.integers(0, nb))
        j = int(rng.integers(0, NC))
        if d["tam"][b, j] < 0:
            continue
        k = 0 if j < NS + 3 else int(d["ts"][b]) + 1
        ci = int(d["cls"][b])
        tc, td = E["THC"][ci, j, k], E["THD"][ci, j, k]
        s = float(d["s"][b, j])
        perf = ctx.perfil(tc, td, s, j == 0)
        for c, arm in CENARIOS:
            for t in CAMADAS:
                ref = ctx.liquido_gb(s, perf, c, arm, t)["liquido_usd_gb"]
                pior = max(pior, abs(ref - d["V"]["%s+%s|%s" % (c, arm, t)][b, j]))
    return pior


def custo_para_rotulo(d, rotulo):
    if rotulo == "tam":
        return d["tam_eff"][:, :NS]
    return -d["V"][rotulo][:, :NS] * d["len"][:, None]


def rotulos_de_custo(C, tol):
    melhor = C.min(1)
    thr = melhor + np.maximum(tol * np.abs(melhor), 1e-9)
    y_tol = np.argmax(C <= thr[:, None], axis=1)
    y_estrito = np.argmin(C, axis=1)
    return y_estrito, y_tol


def pesos_gap(C):
    gap = (C - C.min(1, keepdims=True)).mean(1)
    w = gap + 0.02 * gap.mean() + 1e-12
    return w / w.mean()


QS = np.linspace(0.04, 0.96, 24)


def ajustar_arvore(X, y, w, K, prof=6, min_folha=10):
    feat, thr, esq, dire, cls = [], [], [], [], []
    F = X.shape[1]

    def construir(idx, d):
        nid = len(feat)
        feat.append(-1)
        thr.append(0.0)
        esq.append(-1)
        dire.append(-1)
        yi = y[idx]
        wi = w[idx]
        tot = np.bincount(yi, weights=wi, minlength=K)
        cls.append(int(np.argmax(tot)))
        if d >= prof or np.count_nonzero(tot) <= 1 or len(idx) < 2 * min_folha:
            return nid
        W = tot.sum()
        imp0 = W - float((tot**2).sum()) / W
        melhor = (1e-12, -1, 0.0)
        for f in range(F):
            xs = X[idx, f]
            q = np.unique(np.quantile(xs, QS))
            if len(q) == 0:
                continue
            b = np.searchsorted(q, xs, side="left")
            nb = len(q) + 1
            cw = np.bincount(b * K + yi, weights=wi, minlength=nb * K).reshape(nb, K)
            cn = np.bincount(b, minlength=nb)
            cwl = np.cumsum(cw, 0)[:-1]
            cnl = np.cumsum(cn)[:-1]
            cwr = tot - cwl
            cnr = len(idx) - cnl
            Wl = cwl.sum(1)
            Wr = cwr.sum(1)
            imp = (Wl - (cwl**2).sum(1) / np.maximum(Wl, 1e-12)) + (Wr - (cwr**2).sum(1) / np.maximum(Wr, 1e-12))
            imp[(cnl < min_folha) | (cnr < min_folha)] = np.inf
            j = int(np.argmin(imp))
            ganho = imp0 - imp[j]
            if ganho > melhor[0]:
                melhor = (ganho, f, float(q[j]))
        if melhor[1] < 0:
            return nid
        f, t = melhor[1], melhor[2]
        m = X[idx, f] <= t
        feat[nid] = f
        thr[nid] = t
        esq[nid] = construir(idx[m], d + 1)
        dire[nid] = construir(idx[~m], d + 1)
        return nid

    construir(np.arange(len(y)), 0)
    return {"feat": feat, "thr": thr, "esq": esq, "dir": dire, "cls": cls}


def prever_arvore(m, X):
    feat = np.array(m["feat"])
    thr = np.array(m["thr"])
    esq = np.array(m["esq"])
    dire = np.array(m["dir"])
    cls = np.array(m["cls"])
    no = np.zeros(len(X), np.int64)
    ar = np.arange(len(X))
    for _ in range(64):
        f = feat[no]
        folha = f < 0
        if folha.all():
            break
        go = X[ar, np.maximum(f, 0)] <= thr[no]
        prox = np.where(go, esq[no], dire[no])
        no = np.where(folha, no, prox)
    return cls[no]


def arvore_usa(m):
    return sorted({int(f) for f in m["feat"] if f >= 0})


def ajustar_logit(X, y, w, K, lam=0.01, iters=300, lr=0.08):
    mu = X.mean(0)
    sd = np.maximum(X.std(0), 1e-3)
    Zb = np.hstack([(X - mu) / sd, np.ones((len(X), 1))])
    Y = np.zeros((len(y), K))
    Y[np.arange(len(y)), y] = 1.0
    wn = (w / w.sum())[:, None]
    W = np.zeros((Zb.shape[1], K))
    m1 = np.zeros_like(W)
    m2 = np.zeros_like(W)
    for t in range(1, iters + 1):
        Z = Zb @ W
        Z -= Z.max(1, keepdims=True)
        P = np.exp(Z)
        P /= P.sum(1, keepdims=True)
        G = Zb.T @ ((P - Y) * wn)
        G[:-1] += lam * W[:-1]
        m1 = 0.9 * m1 + 0.1 * G
        m2 = 0.999 * m2 + 0.001 * G * G
        W -= lr * (m1 / (1 - 0.9**t)) / (np.sqrt(m2 / (1 - 0.999**t)) + 1e-8)
    W[-1, np.bincount(y, minlength=K) == 0] = -1e9
    return {"mu": mu.tolist(), "sd": sd.tolist(), "W": W.tolist()}


def prever_logit(m, X):
    Zb = np.hstack([(X - np.array(m["mu"])) / np.array(m["sd"]), np.ones((len(X), 1))])
    return np.argmax(Zb @ np.array(m["W"]), axis=1)


def plano_regra(X):
    h0 = X[:, FIDX["h0"]]
    marg = np.stack([h0 - X[:, FIDX[n]] for n in ("hd1", "hd2", "hd4", "hd8", "hp4", "hp8")], 1)
    plano = np.full(len(X), 2, np.int64)
    am = np.argmax(marg, 1)
    usar = marg[np.arange(len(X)), am] >= MARGEM_REGRA
    plano[usar] = 3 + am[usar]
    plano[h0 > H0_STORED] = 0
    return plano


def plano_probe(X):
    return np.where(X[:, FIDX["pr"]] > LIMIAR_PROBE, 0, 2)


def ajustar_e_prever(modelo, X, y, w, K, Xt):
    if modelo == "cart":
        m = ajustar_arvore(X, y, w, K)
        return prever_arvore(m, Xt), m
    m = ajustar_logit(X, y, w, K)
    return prever_logit(m, Xt), m


def validacao_cruzada(modelo, X, y, w, grupos, K):
    plano = np.zeros(len(y), np.int64)
    for gr in np.unique(grupos):
        te = grupos == gr
        p, _ = ajustar_e_prever(modelo, X[~te], y[~te], w[~te], K, X[te])
        plano[te] = p
    return plano


DG = {}


def tarefa_modelos(t):
    g, rotulo, vi, pesado = t
    d = DG[g]
    X = d["X"][:, vi, :].astype(np.float64)
    C = custo_para_rotulo(d, rotulo)
    y_est, y_tol = rotulos_de_custo(C, 0.003)
    w = pesos_gap(C) if pesado else np.ones(len(y_tol))
    saida = {}
    for modelo in ("cart", "logit"):
        lofo = validacao_cruzada(modelo, X, y_tol, w, d["arq"], NS)
        loco = validacao_cruzada(modelo, X, y_tol, w, d["cls"], NS)
        _, m = ajustar_e_prever(modelo, X, y_tol, w, NS, X[:2])
        saida[modelo] = {"lofo": lofo, "loco": loco, "final": m}
    return (g, rotulo), saida


def tarefa_triagem(t):
    g, vi, pesado = t
    d = DG[g]
    X = d["X"][:, vi, :].astype(np.float64)
    C = custo_para_rotulo(d, "tam")
    _, y_tol = rotulos_de_custo(C, 0.003)
    w = pesos_gap(C) if pesado else np.ones(len(y_tol))
    plano = validacao_cruzada("cart", X, y_tol, w, d["arq"], NS)
    ar = np.arange(len(plano))
    by = d["tam_eff"][ar, plano].sum()
    orc = d["tam_eff"][:, :NS].min(1).sum()
    return (g, vi, pesado), {"regret_bytes_pct": float((by - orc) / orc * 100), "acc": float((plano == np.argmin(C, 1)).mean())}


def carregar_dados(rot, feat):
    R = np.load(rot)
    Fz = np.load(feat)
    nomes = R["nomes"].tolist()
    classes_arq = R["classes"].tolist()
    classes = [c for c in ORDEM_CLASSES if c in set(classes_arq)]
    cidx = {c: i for i, c in enumerate(classes)}
    D = {}
    for g, _ in GRANS:
        arq = R[g + "_arq"].astype(np.int64)
        X = Fz[g + "_X"]
        if len(X) != len(arq):
            sys.exit("features e rotulos desalinhados em " + g)
        D[g] = {
            "arq": arq,
            "off": R[g + "_off"],
            "len": R[g + "_len"],
            "tam": R[g + "_tam"],
            "ok": R[g + "_ok"],
            "ts": R[g + "_ts"].astype(np.int64),
            "X": X,
            "cls": np.array([cidx[classes_arq[i]] for i in arq]),
        }
    return D, nomes, classes_arq, classes


def somas_por_classe(d, v, ncl):
    return np.bincount(d["cls"], weights=v, minlength=ncl)


def resumo_plano(d, plano, orc, ncl, classes):
    ar = np.arange(len(plano))
    L = d["len"].astype(np.float64)
    by = d["tam_eff"][ar, plano]
    vp = {r: d["V"][r][ar, plano] for r in ROTULOS_ECO}
    bp = somas_por_classe(d, by, ncl)
    bo = somas_por_classe(d, orc["bytes"], ncl)
    Ls = somas_por_classe(d, L, ncl)
    sp = {r: somas_por_classe(d, vp[r] * L, ncl) for r in ROTULOS_ECO}
    so = {r: somas_por_classe(d, orc["V"][r] * L, ncl) for r in ROTULOS_ECO}

    def montar(bpv, bov, Lv, spv, sov):
        return {
            "bytes": float(bpv),
            "bytes_oraculo": float(bov),
            "bytes_logicos": float(Lv),
            "regret_bytes_pct": float((bpv - bov) / bov * 100.0) if bov > 0 else 0.0,
            "razao_x": float(Lv / bpv) if bpv > 0 else 0.0,
            "valor_usd_gb": {r: float(spv[r] / Lv) for r in ROTULOS_ECO},
            "regret_usd_gb": {r: float((sov[r] - spv[r]) / Lv) for r in ROTULOS_ECO},
        }

    saida = {"global": montar(bp.sum(), bo.sum(), Ls.sum(), {r: sp[r].sum() for r in ROTULOS_ECO}, {r: so[r].sum() for r in ROTULOS_ECO}), "por_classe": {}}
    for ci, cl in enumerate(classes):
        saida["por_classe"][cl] = montar(bp[ci], bo[ci], Ls[ci], {r: sp[r][ci] for r in ROTULOS_ECO}, {r: so[r][ci] for r in ROTULOS_ECO})
    nf = int(d["arq"].max()) + 1
    saida["por_arquivo"] = {"plano": np.bincount(d["arq"], weights=by, minlength=nf).tolist(), "oraculo": np.bincount(d["arq"], weights=orc["bytes"], minlength=nf).tolist()}
    return saida


def acuracias(d, plano, orcs, classes):
    saida = {}
    ar = np.arange(len(plano))
    for rotulo, (idx, C) in orcs.items():
        melhor = C[ar, idx]
        tol = melhor + np.maximum(0.005 * np.abs(melhor), 1e-9)
        est = plano == idx
        tolr = C[ar, plano] <= tol
        entrada = {"estrita": float(est.mean()), "tolerante": float(tolr.mean()), "por_classe": {}}
        for ci, cl in enumerate(classes):
            m = d["cls"] == ci
            entrada["por_classe"][cl] = {"estrita": float(est[m].mean()), "tolerante": float(tolr[m].mean())}
        saida[rotulo] = entrada
    return saida


def matriz_confusao(idx, plano):
    M = np.zeros((NS, NS), np.int64)
    np.add.at(M, (idx, plano), 1)
    return M.tolist()


def erros_por_arquivo(d, plano, nomes):
    ar = np.arange(len(plano))
    orc = d["tam_eff"][:, :NS].min(1)
    extra = d["tam_eff"][ar, plano] - orc
    saida = []
    for fi in np.unique(d["arq"]):
        m = d["arq"] == fi
        saida.append(
            {
                "arquivo": nomes[fi],
                "blocos": int(m.sum()),
                "bytes_extra": float(extra[m].sum()),
                "regret_pct": float(extra[m].sum() / orc[m].sum() * 100.0),
                "acc_estrita": float((plano[m] == np.argmin(d["tam_eff"][m][:, :NS], 1)).mean()),
            }
        )
    saida.sort(key=lambda x: -x["bytes_extra"])
    return saida


def analise_transforms(d, classes, E):
    ncl = len(classes)
    z3 = d["tam_eff"][:, 2]
    tt = d["tam_eff"][:, 3:NS]
    melhor_t = tt.min(1)
    saida = {"por_classe": {}}
    for ci, cl in enumerate(classes):
        m = d["cls"] == ci
        entrada = {
            "bytes_zstd3": float(z3[m].sum()),
            "bytes_melhor_transform": float(np.minimum(melhor_t[m], z3[m]).sum()),
            "ganho_pct_vs_zstd3": float((1.0 - np.minimum(melhor_t[m], z3[m]).sum() / z3[m].sum()) * 100.0),
            "blocos_transform_vence_3pct": float((melhor_t[m] < 0.97 * z3[m]).mean()),
            "por_transform": {},
        }
        for ti, (nome, _, _) in enumerate(TRANSFORMS):
            entrada["por_transform"][nome] = {
                "ganho_pct_vs_zstd3": float((1.0 - tt[m, ti].sum() / z3[m].sum()) * 100.0),
                "vazao_comp_MBps": float(E["THC"][ci, 3 + ti, 0]),
                "vazao_decomp_MBps": float(E["THD"][ci, 3 + ti, 0]),
            }
        entrada["vazao_zstd3_comp_MBps"] = float(E["THC"][ci, 2, 0])
        entrada["vazao_zstd3_decomp_MBps"] = float(E["THD"][ci, 2, 0])
        mt = m & (d["ts"] >= 0)
        if mt.any():
            xz = d["tam_eff"][:, NS + 1]
            z19 = d["tam_eff"][:, NS]
            entrada["async_T_blocos"] = int(mt.sum())
            entrada["async_T_xz_ganho_pct_vs_xz"] = float((1.0 - d["tam_eff"][mt, NS + 4].sum() / xz[mt].sum()) * 100.0)
            entrada["async_T_zstd19_ganho_pct_vs_zstd19"] = float((1.0 - d["tam_eff"][mt, NS + 3].sum() / z19[mt].sum()) * 100.0)
        saida["por_classe"][cl] = entrada
    return saida


def cmd_avaliar(a):
    t0 = time.perf_counter()
    D, nomes, classes_arq, classes = carregar_dados(a.rotulos, a.features)
    ncl = len(classes)
    E = preparar_economia(a.medicoes, a.precos, a.transformes, a.custo_transform, classes)
    saida = {"custo_transform": a.custo_transform, "classes": classes, "arquivos": nomes, "classes_arquivos": classes_arq, "granularidades": {}}
    verif = 0.0
    for g, _ in GRANS:
        construir_valores(D[g], E)
        verif = max(verif, verificar_valores_contra_economia(D[g], E))
    saida["verificacao_valor_vs_economia_py_max_abs_usd_gb"] = verif
    DG.update(D)

    ctx = mp.get_context("fork")
    cont = ctx.Value("i", 0)
    triagem = []
    with ctx.Pool(6, initializer=iniciar_trabalhador, initargs=(cont, CPUS_ROTULO)) as pool:
        tt = [(g, vi, ps) for g, _ in GRANS for vi in range(len(VARIANTES)) for ps in (False, True)]
        for (g, vi, ps), r in pool.imap_unordered(tarefa_triagem, tt, chunksize=1):
            triagem.append({"gran": g, "variante": vi, "politica": VARIANTES[vi][0], "amostra": VARIANTES[vi][1], "pesos": ps, **r})
    print("triagem pronta %.0fs" % (time.perf_counter() - t0), flush=True)
    triagem.sort(key=lambda x: (x["gran"], x["variante"], x["pesos"]))
    saida["triagem"] = triagem
    medias = {}
    for t in triagem:
        medias.setdefault((t["variante"], t["pesos"]), []).append(t["regret_bytes_pct"])
    cand = sorted(((np.mean(v) + 0.05 * (VARIANTES[k[0]][1] == 16384), k) for k, v in medias.items()))
    vi, pesado = cand[0][1]
    saida["variante_idx"] = vi
    saida["variante"] = {"politica": VARIANTES[vi][0], "amostra_bytes": VARIANTES[vi][1]}
    saida["pesos_escolhidos"] = pesado
    saida["criterio_escolha_variante"] = "menor regret medio de bytes (CART, LOFO, rotulo tamanho) nas 3 granularidades, com +0,05 ponto de penalidade para amostra de 16 KiB"

    rotulos = ["tam"] + ROTULOS_ECO
    plans = {}
    finais = {g: {} for g, _ in GRANS}
    with ctx.Pool(6, initializer=iniciar_trabalhador, initargs=(cont, CPUS_ROTULO)) as pool:
        tt = [(g, r, vi, pesado) for g, _ in GRANS for r in rotulos]
        for (g, r), res in pool.imap_unordered(tarefa_modelos, tt, chunksize=1):
            for modelo, v in res.items():
                plans[(g, r, modelo, "lofo")] = v["lofo"]
                plans[(g, r, modelo, "loco")] = v["loco"]
                finais[g].setdefault(r, {})[modelo] = v["final"]
    print("modelos prontos %.0fs" % (time.perf_counter() - t0), flush=True)
    saida["modelos_finais"] = finais

    for g, gran in GRANS:
        d = D[g]
        n = len(d["len"])
        ar = np.arange(n)
        orc_idx = {"tam": np.argmin(d["tam_eff"][:, :NS], 1)}
        orcs = {"tam": (orc_idx["tam"], d["tam_eff"][:, :NS])}
        for r in ROTULOS_ECO:
            orc_idx[r] = np.argmax(d["V"][r][:, :NS], 1)
            orcs[r] = (orc_idx[r], -d["V"][r][:, :NS])
        orc = {"bytes": d["tam_eff"][ar, orc_idx["tam"]], "V": {r: d["V"][r][ar, orc_idx[r]] for r in ROTULOS_ECO}}
        orc_eco_plano = {r: orc_idx[r] for r in ROTULOS_ECO}
        base = {}
        X0 = d["X"][:, 0, :].astype(np.float64)
        X1 = d["X"][:, 1, :].astype(np.float64)
        XV = d["X"][:, vi, :].astype(np.float64)
        base["zstd-3 fixo"] = np.full(n, 2)
        base["zstd-1 fixo"] = np.full(n, 1)
        base["probe+zstd-3 (inicio)"] = plano_probe(X0)
        base["probe+zstd-3 (espalhado)"] = plano_probe(X1)
        base["regra manual"] = plano_regra(XV)
        base["oraculo (tamanho)"] = orc_idx["tam"]
        gran_out = {"n_blocos": int(n), "bytes_logicos": float(d["len"].sum()), "linhas_base": {}, "modelos": {}, "confusao": {}, "erros_por_arquivo": {}}
        for nome, plano in base.items():
            gran_out["linhas_base"][nome] = {"resumo": resumo_plano(d, plano, orc, ncl, classes), "acuracia": acuracias(d, plano, orcs, classes)}
        for r in ROTULOS_ECO:
            orc_r = {"bytes": d["tam_eff"][ar, orc_idx[r]], "V": orc["V"]}
            gran_out["linhas_base"]["oraculo (" + r + ")"] = {"resumo": resumo_plano(d, orc_idx[r], orc, ncl, classes), "acuracia": acuracias(d, orc_idx[r], orcs, classes)}
        medir = {"V": {}, "tempo_sync_sobre_zstd3": float(d["tc"][:, :NS].sum() / d["tc"][:, 2].sum()), "tempo_todos_sobre_zstd3": float(np.where(d["tam"] >= 0, d["tc"], 0.0).sum() / d["tc"][:, 2].sum()), "por_classe_sync_sobre_zstd3": {}}
        for ci, cl in enumerate(classes):
            m = d["cls"] == ci
            medir["por_classe_sync_sobre_zstd3"][cl] = float(d["tc"][m][:, :NS].sum() / d["tc"][m][:, 2].sum())
        vm = {}
        for r in ROTULOS_ECO:
            cpu = r.split("+")[0]
            extra = d["cc"][cpu][:, :NS].sum(1) - d["cc"][cpu][ar, orc_idx[r]]
            vm[r] = orc["V"][r] - extra
        dm = dict(d)
        dm["V"] = {r: np.tile(vm[r][:, None], (1, NC)) for r in ROTULOS_ECO}
        dm["tam_eff"] = np.tile(orc["bytes"][:, None], (1, NC))
        gran_out["linhas_base"]["medir todos"] = {"resumo": resumo_plano(dm, np.zeros(n, np.int64), orc, ncl, classes), "tempo": medir}
        mascara_async = np.isin(np.arange(NC), [0] + list(range(NS, NC)))
        orc_async = np.argmin(np.where(mascara_async[None, :], d["tam_eff"], np.inf), 1)
        orc_tudo = np.argmin(d["tam_eff"], 1)
        gran_out["oraculo_async"] = {
            "bytes": float(d["tam_eff"][ar, orc_async].sum()),
            "bytes_oraculo_sync": float(orc["bytes"].sum()),
            "bytes_oraculo_tudo": float(d["tam_eff"][ar, orc_tudo].sum()),
            "bytes_zstd3": float(d["tam_eff"][:, 2].sum()),
            "distribuicao_por_classe": {cl: {CANDS[j]: int(((orc_async == j) & (d["cls"] == ci)).sum()) for j in range(NC) if ((orc_async == j) & (d["cls"] == ci)).any()} for ci, cl in enumerate(classes)},
            "bytes_por_candidato_por_classe": {cl: {CANDS[j]: float(d["tam_eff"][d["cls"] == ci][:, j][np.isfinite(d["tam_eff"][d["cls"] == ci][:, j])].sum()) for j in (2, NS, NS + 1, NS + 2)} for ci, cl in enumerate(classes)},
        }
        gran_out["oraculo_distribuicao_sync_por_classe"] = {cl: {CANDS[j]: int(((orc_idx["tam"] == j) & (d["cls"] == ci)).sum()) for j in range(NS)} for ci, cl in enumerate(classes)}
        gran_out["oraculo_eco_distribuicao"] = {r: {CANDS[j]: int((orc_idx[r] == j).sum()) for j in range(NS)} for r in ROTULOS_ECO}
        for modelo in ("cart", "logit"):
            for r in rotulos:
                for esq in ("lofo", "loco"):
                    plano = plans[(g, r, modelo, esq)]
                    entrada = {"resumo": resumo_plano(d, plano, orc, ncl, classes), "acuracia": acuracias(d, plano, orcs, classes)}
                    gran_out["modelos"].setdefault(modelo, {}).setdefault(r, {})[esq] = entrada
                    if esq == "lofo" and r in ("tam", "nuvem+s3_standard|morno"):
                        gran_out["confusao"].setdefault(modelo, {})[r] = matriz_confusao(orc_idx[r], plano)
                        if r == "tam":
                            gran_out["erros_por_arquivo"][modelo] = erros_por_arquivo(d, plano, nomes)
                m = finais[g][r][modelo]
                gran_out["modelos"][modelo][r]["tamanho"] = {"nos": len(m["feat"]), "folhas": int(sum(1 for f in m["feat"] if f < 0)), "features_usadas": [FEATS[i] for i in arvore_usa(m)]} if modelo == "cart" else {"pesos": len(np.array(m["W"]).ravel()), "forma": list(np.array(m["W"]).shape)}
        gran_out["confusao"]["probe_inicio"] = {"tam": matriz_confusao(orc_idx["tam"], base["probe+zstd-3 (inicio)"])}
        gran_out["confusao"]["regra"] = {"tam": matriz_confusao(orc_idx["tam"], base["regra manual"])}
        gran_out["confusao"]["probe_espalhado"] = {"tam": matriz_confusao(orc_idx["tam"], base["probe+zstd-3 (espalhado)"])}
        gran_out["transforms"] = analise_transforms(d, classes, E)
        gran_out["elegibilidade_sincrona_vazao_comp_MBps"] = {cl: {CANDS[j]: float(E["THC"][ci, j, 0]) for j in range(NS)} for ci, cl in enumerate(classes)}
        gran_out["bit_exatidao"] = {"comparacoes": int((d["tam"] >= 0).sum()), "nao_exatas": int(((d["tam"] >= 0) & ~d["ok"]).sum())}
        saida["granularidades"][g] = gran_out
        print("gran", g, "resumida %.0fs" % (time.perf_counter() - t0), flush=True)
    saida["tempo_s"] = time.perf_counter() - t0
    with open(a.saida, "w") as f:
        json.dump(saida, f)
    print("avaliacao gravada", a.saida, "%.0fs" % saida["tempo_s"])


def arvore_para_lista(m):
    return m["feat"], m["thr"], m["esq"], m["dir"], m["cls"]


def cmd_tempo(a):
    os.sched_setaffinity(0, {CPU_TEMPO})
    gc.disable()
    with open(a.avaliacao) as f:
        av = json.load(f)
    vi = av["variante_idx"]
    pol, S = VARIANTES[vi]
    corpus = listar_corpus(a.dados, a.kernel)
    saida = {"cpu": CPU_TEMPO, "variante": av["variante"], "repeticoes": 3, "estatistica": "mediana de 3 por bloco", "por_gran": {}}
    for g, gran in GRANS:
        fin = av["modelos_finais"][g]["tam"]
        arv = fin["cart"]
        usados = [FEATS[i] for i in arvore_usa(arv)]
        ft, th, es, di, cl = arvore_para_lista(arv)
        lg = fin["logit"]
        mu = np.array(lg["mu"])
        sd = np.array(lg["sd"])
        W = np.array(lg["W"])

        def inferir_arvore(v):
            n = 0
            while ft[n] >= 0:
                n = es[n] if v[ft[n]] <= th[n] else di[n]
            return cl[n]

        def inferir_logit(v):
            return int(np.argmax(((v - mu) / sd) @ W[:-1] + W[-1]))

        acum = {}
        t_ini = time.perf_counter()
        for fi, arq in enumerate(corpus):
            with open(arq["caminho"], "rb") as f:
                dados = f.read()
            c = acum.setdefault(arq["classe"], {"n": 0, "bytes": 0, "z3": 0.0, "z1": 0.0, "full": 0.0, "sub": 0.0, "probe": 0.0, "regra": 0.0, "inf_cart": 0.0, "inf_logit": 0.0, "r_full": [], "r_sub": []})
            for off in range(0, len(dados), gran):
                blk = dados[off : off + gran]
                x = np.frombuffer(blk, dtype=np.uint8)
                tz3 = mediana_ns(lambda: zstd.compress(blk, level=3))
                tz1 = mediana_ns(lambda: zstd.compress(blk, level=1))
                tfull = mediana_ns(lambda: extrair(x, pol, S, gran, FEATS))
                tsub = mediana_ns(lambda: extrair(x, pol, S, gran, usados))
                tpr = mediana_ns(lambda: extrair(x, pol, S, gran, ["pr"]))
                tre = mediana_ns(lambda: extrair(x, pol, S, gran, SUBCONJ_REGRA))
                v = np.array(extrair(x, pol, S, gran, FEATS))
                vl = v.tolist()
                tic = mediana_ns(lambda: inferir_arvore(vl))
                til = mediana_ns(lambda: inferir_logit(v))
                c["n"] += 1
                c["bytes"] += len(blk)
                c["z3"] += tz3
                c["z1"] += tz1
                c["full"] += tfull
                c["sub"] += tsub
                c["probe"] += tpr
                c["regra"] += tre
                c["inf_cart"] += tic
                c["inf_logit"] += til
                c["r_full"].append((tfull + til) / tz3)
                c["r_sub"].append((tsub + tic) / tz3)
        g_out = {"features_usadas_cart": usados, "por_classe": {}, "duracao_s": time.perf_counter() - t_ini}
        tot = {"n": 0, "bytes": 0, "z3": 0.0, "z1": 0.0, "full": 0.0, "sub": 0.0, "probe": 0.0, "regra": 0.0, "inf_cart": 0.0, "inf_logit": 0.0}
        rf, rs = [], []
        for cl_, c in acum.items():
            for k in tot:
                tot[k] += c[k]
            rf += c["r_full"]
            rs += c["r_sub"]
            g_out["por_classe"][cl_] = resumo_tempo(c)
        t = resumo_tempo(tot)
        t["mediana_razao_por_bloco_full_logit_pct"] = float(np.median(rf) * 100.0)
        t["mediana_razao_por_bloco_sub_cart_pct"] = float(np.median(rs) * 100.0)
        g_out["global"] = t
        saida["por_gran"][g] = g_out
        print(g, "tempo medido em %.0fs" % g_out["duracao_s"], "features full %.1f us/bloco  sub %.1f us  zstd-3 %.1f us" % (t["us_features_full"], t["us_features_sub_cart"], t["us_zstd3"]), flush=True)
    with open(a.saida, "w") as f:
        json.dump(saida, f, indent=1)


def resumo_tempo(c):
    n = c["n"]
    return {
        "blocos": n,
        "us_zstd3": c["z3"] / n / 1e3,
        "us_zstd1": c["z1"] / n / 1e3,
        "us_features_full": c["full"] / n / 1e3,
        "us_features_sub_cart": c["sub"] / n / 1e3,
        "us_probe": c["probe"] / n / 1e3,
        "us_inferencia_cart": c["inf_cart"] / n / 1e3,
        "us_inferencia_logit": c["inf_logit"] / n / 1e3,
        "pct_zstd3_full_mais_logit": (c["full"] + c["inf_logit"]) / c["z3"] * 100.0,
        "pct_zstd3_sub_mais_cart": (c["sub"] + c["inf_cart"]) / c["z3"] * 100.0,
        "pct_zstd3_probe": c["probe"] / c["z3"] * 100.0,
        "us_features_regra": c["regra"] / n / 1e3,
        "pct_zstd3_regra": c["regra"] / c["z3"] * 100.0,
        "pct_zstd3_full": c["full"] / c["z3"] * 100.0,
        "pct_zstd3_sub": c["sub"] / c["z3"] * 100.0,
    }


def n(x, nd=2):
    if x is None:
        return "-"
    return ("%.*f" % (nd, x)).replace(".", ",")


def tabela(cab, linhas):
    s = "| " + " | ".join(cab) + " |\n|" + "|".join(["---"] * len(cab)) + "|\n"
    for l in linhas:
        s += "| " + " | ".join(str(c) for c in l) + " |\n"
    return s


CURTO = ["stored", "z1", "z3", "d1", "d2", "d4", "d8", "b4", "b8"]
BASES = ["zstd-3 fixo", "zstd-1 fixo", "probe+zstd-3 (inicio)", "probe+zstd-3 (espalhado)", "regra manual"]
GRUPOS_PASSOS = {"h0": "hist", "nd": "hist", "fz": "hist", "fp": "hist", "fmax": "hist", "h1": "h1", "run": "run", "hd1": "hd1", "hd2": "hd2", "hd4": "hd4", "hd8": "hd8", "hp4": "hp4", "hp8": "hp8", "ac2": "ac2", "ac4": "ac4", "ac8": "ac8", "cr2": "cr2", "cr4": "cr4", "cr8": "cr8"}
BYTES_POR_US_NATIVO = 2000.0


def passos_nativos(nomes):
    gs = {GRUPOS_PASSOS[x] for x in nomes if x in GRUPOS_PASSOS}
    if any(x.startswith("cr") for x in gs):
        gs.add("media")
    return len(gs)


def us_nativo(nomes, S, us_probe):
    return passos_nativos(nomes) * S / BYTES_POR_US_NATIVO + (us_probe if "pr" in nomes else 0.0)


def bootstrap(pf_a, pf_o, pf_ref=None, B=2000, seed=5):
    rng = np.random.default_rng(seed)
    a = np.array(pf_a)
    o = np.array(pf_o)
    nf = len(a)
    idx = rng.integers(0, nf, (B, nf))
    reg = (a[idx].sum(1) - o[idx].sum(1)) / o[idx].sum(1) * 100.0
    out = {"regret_pct_med": float(np.median(reg)), "regret_pct_ic95": [float(np.percentile(reg, 2.5)), float(np.percentile(reg, 97.5))]}
    if pf_ref is not None:
        r = np.array(pf_ref)
        gan = (r[idx].sum(1) - a[idx].sum(1)) / r[idx].sum(1) * 100.0
        out["ganho_vs_ref_pct_med"] = float(np.median(gan))
        out["ganho_vs_ref_pct_ic95"] = [float(np.percentile(gan, 2.5)), float(np.percentile(gan, 97.5))]
        out["prob_ganho_ge_1pct"] = float((gan >= 1.0).mean())
    out["prob_regret_le_1pct"] = float((reg <= 1.0).mean())
    return out


def criterio_parada(av, tempo):
    S = av["variante"]["amostra_bytes"]
    saida = {}
    for g, _ in GRANS:
        go = av["granularidades"][g]
        tg = tempo["por_gran"][g]["global"]
        usados = tempo["por_gran"][g]["features_usadas_cart"]
        probes = {k: go["linhas_base"][k]["resumo"] for k in ("probe+zstd-3 (inicio)", "probe+zstd-3 (espalhado)")}
        melhor_probe = min(probes, key=lambda k: probes[k]["global"]["bytes"])
        pr = probes[melhor_probe]
        entrada = {"probe_usado": melhor_probe, "probe_regret_bytes_pct": pr["global"]["regret_bytes_pct"], "probe_dentro_de_1pct_do_oraculo": pr["global"]["regret_bytes_pct"] <= 1.0, "modelos": {}}
        for m in ("cart", "logit", "regra"):
            if m == "regra":
                res = go["linhas_base"]["regra manual"]["resumo"]
                custo_num = tg["pct_zstd3_regra"]
                custo_nat = us_nativo(SUBCONJ_REGRA, S, tg["us_probe"]) / tg["us_zstd3"] * 100.0
            else:
                res = go["modelos"][m]["tam"]["lofo"]["resumo"]
                if m == "cart":
                    custo_num = tg["pct_zstd3_sub_mais_cart"]
                    custo_nat = us_nativo(usados, S, tg["us_probe"]) / tg["us_zstd3"] * 100.0
                else:
                    custo_num = tg["pct_zstd3_full_mais_logit"]
                    custo_nat = us_nativo(FEATS, S, tg["us_probe"]) / tg["us_zstd3"] * 100.0
            bs = bootstrap(res["por_arquivo"]["plano"], res["por_arquivo"]["oraculo"], pr["por_arquivo"]["plano"])
            reg = res["global"]["regret_bytes_pct"]
            gan = (pr["global"]["bytes"] - res["global"]["bytes"]) / pr["global"]["bytes"] * 100.0
            entrada["modelos"][m] = {
                "i_regret_bytes_pct": reg,
                "i_ok": reg <= 1.0,
                "ii_ganho_vs_probe_pct": gan,
                "ii_ok": gan >= 1.0,
                "iii_custo_pct_zstd3_numpy_medido": custo_num,
                "iii_ok_numpy": custo_num <= 2.0,
                "iii_custo_pct_zstd3_nativo_estimado": custo_nat,
                "iii_ok_nativo_estimado": custo_nat <= 2.0,
                "bootstrap_arquivos": bs,
                "todas_ok_medido": reg <= 1.0 and gan >= 1.0 and custo_num <= 2.0,
                "todas_ok_com_nativo_estimado": reg <= 1.0 and gan >= 1.0 and custo_nat <= 2.0,
            }
        cls = {m: entrada["modelos"][m] for m in ("cart", "logit")}
        melhor = min(cls, key=lambda m: cls[m]["i_regret_bytes_pct"])
        entrada["melhor_classificador"] = melhor
        if entrada["probe_dentro_de_1pct_do_oraculo"]:
            entrada["veredito"] = "NAO VALE: usar so o probe (probe+zstd-3 a <= 1% do oraculo)"
        elif cls[melhor]["todas_ok_medido"]:
            entrada["veredito"] = "VALE (as 3 condicoes passam com os numeros medidos)"
        else:
            falhas = [c for c, ok in (("(i) regret de bytes", cls[melhor]["i_ok"]), ("(ii) ganho sobre o probe", cls[melhor]["ii_ok"]), ("(iii) custo de features", cls[melhor]["iii_ok_numpy"])) if not ok]
            entrada["veredito"] = "NAO VALE com os numeros medidos: reprova " + ", ".join(falhas)
        saida[g] = entrada
    return saida


def md_oraculo(av):
    s = ""
    for g, _ in GRANS:
        go = av["granularidades"][g]
        linhas = []
        for cl in av["classes"]:
            d = go["oraculo_distribuicao_sync_por_classe"][cl]
            tot = sum(d.values())
            linhas.append([cl, tot] + [n(d[c] / tot * 100, 0) + "%" if d[c] else "0" for c in SYNC])
        s += "Granularidade %s (oraculo de tamanho do caminho sincrono; %% dos blocos da classe cujo menor tamanho e do plano):\n\n" % g + tabela(["classe", "blocos"] + CURTO, linhas) + "\n"
    return s


def md_async(av):
    s = ""
    for g, _ in GRANS:
        go = av["granularidades"][g]["oraculo_async"]
        s += "- %s: oraculo assincrono = %s B; oraculo sincrono = %s B; zstd-3 fixo = %s B; oraculo sincrono ou assincrono = %s B. Ganho do assincrono sobre o sincrono: %s%%.\n" % (g, n(go["bytes"], 0), n(go["bytes_oraculo_sync"], 0), n(go["bytes_zstd3"], 0), n(go["bytes_oraculo_tudo"], 0), n((1 - go["bytes"] / go["bytes_oraculo_sync"]) * 100, 2))
    linhas = []
    go = av["granularidades"]["256K"]["oraculo_async"]
    for cl in av["classes"]:
        b = go["bytes_por_candidato_por_classe"][cl]
        ref = b["zstd-3"]
        dist = go["distribuicao_por_classe"][cl]
        tot = sum(dist.values())
        venc = max(dist, key=dist.get)
        linhas.append([cl, n(ref / 1e6, 2)] + [n((1 - b[c] / ref) * 100, 1) + "%" for c in ("zstd-19", "xz-9e", "bzip2-9")] + [venc + " (" + n(dist[venc] / tot * 100, 0) + "%)"])
    s += "\nBytes por candidato assincrono (256K), reducao % sobre zstd-3 da mesma classe e o plano que mais vence:\n\n" + tabela(["classe", "zstd-3 (MB)", "zstd-19", "xz-9e", "bzip2-9", "vence mais"], linhas)
    return s


def linha_regret(nome, r):
    g = r["global"]
    return [nome, n(g["regret_bytes_pct"], 2) + "%"] + [n(g["regret_usd_gb"][x], 5) for x in ROTULOS_ECO]


def md_regret_global(av):
    s = ""
    cab = ["plano", "regret bytes"] + ["US$/GB " + x.replace("nuvem+s3_standard", "nuvem").replace("onprem+onprem", "on-prem") for x in ROTULOS_ECO]
    for g, _ in GRANS:
        go = av["granularidades"][g]
        linhas = []
        for b in BASES:
            linhas.append(linha_regret(b, go["linhas_base"][b]["resumo"]))
        for m, nm in (("cart", "CART prof. 6"), ("logit", "regressao logistica")):
            linhas.append(linha_regret(nm + " (treino: tamanho)", go["modelos"][m]["tam"]["lofo"]["resumo"]))
            g_ = go["modelos"][m]
            diag = [n(g_[r]["lofo"]["resumo"]["global"]["regret_usd_gb"][r], 5) for r in ROTULOS_ECO]
            linhas.append([nm + " (treino: economico, diagonal)", n(np.nan, 0)] + diag)
        linhas.append(linha_regret("medir todos (custo de medir incluido)", go["linhas_base"]["medir todos"]["resumo"]))
        linhas.append(linha_regret("oraculo (tamanho)", go["linhas_base"]["oraculo (tamanho)"]["resumo"]))
        s += "Granularidade %s (%d blocos, validacao leave-one-file-out; regret em US$ por GB logico, positivo = perdeu valor liquido vs o oraculo economico do mesmo cenario e camada):\n\n" % (g, go["n_blocos"]) + tabela(cab, linhas) + "\n"
    return s.replace("nan", "-")


def md_regret_classe(av):
    s = ""
    nomes = BASES + ["CART", "logistica"]
    for g, _ in GRANS:
        go = av["granularidades"][g]
        linhas = []
        for cl in av["classes"]:
            cel = []
            for b in BASES:
                cel.append(n(go["linhas_base"][b]["resumo"]["por_classe"][cl]["regret_bytes_pct"], 2))
            for m in ("cart", "logit"):
                cel.append(n(go["modelos"][m]["tam"]["lofo"]["resumo"]["por_classe"][cl]["regret_bytes_pct"], 2))
            linhas.append([cl] + cel)
        r = ["GLOBAL"] + [n(go["linhas_base"][b]["resumo"]["global"]["regret_bytes_pct"], 2) for b in BASES] + [n(go["modelos"][m]["tam"]["lofo"]["resumo"]["global"]["regret_bytes_pct"], 2) for m in ("cart", "logit")]
        linhas.append(r)
        med = ["MEDIA DAS CLASSES"]
        for b_ in BASES:
            med.append(n(np.mean([go["linhas_base"][b_]["resumo"]["por_classe"][c]["regret_bytes_pct"] for c in av["classes"]]), 2))
        for m in ("cart", "logit"):
            med.append(n(np.mean([go["modelos"][m]["tam"]["lofo"]["resumo"]["por_classe"][c]["regret_bytes_pct"] for c in av["classes"]]), 2))
        linhas.append(med)
        s += "Granularidade %s, regret de bytes (%%) vs oraculo de tamanho do caminho sincrono, por classe:\n\n" % g + tabela(["classe"] + nomes, linhas) + "\n"
        linhas = []
        for cl in av["classes"]:
            cel = []
            for b in BASES:
                cel.append(n(go["linhas_base"][b]["resumo"]["por_classe"][cl]["regret_usd_gb"]["nuvem+s3_standard|morno"], 5))
            for m in ("cart", "logit"):
                cel.append(n(go["modelos"][m]["nuvem+s3_standard|morno"]["lofo"]["resumo"]["por_classe"][cl]["regret_usd_gb"]["nuvem+s3_standard|morno"], 5))
            linhas.append([cl] + cel)
        s += "Granularidade %s, regret em US$/GB (nuvem+S3 Standard, camada morna; modelos treinados com o rotulo economico dessa celula):\n\n" % g + tabela(["classe"] + nomes, linhas) + "\n"
    return s


def md_acuracia(av):
    s = ""
    for g, _ in GRANS:
        go = av["granularidades"][g]
        linhas = []
        itens = [(b, go["linhas_base"][b]["acuracia"]) for b in BASES]
        for m, nm in (("cart", "CART"), ("logit", "logistica")):
            itens.append((nm + " LOFO", {r: go["modelos"][m][r]["lofo"]["acuracia"][r] for r in ["tam", "nuvem+s3_standard|morno", "onprem+onprem|morno"]}))
            itens.append((nm + " LOCO", {r: go["modelos"][m][r]["loco"]["acuracia"][r] for r in ["tam", "nuvem+s3_standard|morno", "onprem+onprem|morno"]}))
        for nome, ac in itens:
            linhas.append([nome] + [n(ac[r]["estrita"] * 100, 1) + " / " + n(ac[r]["tolerante"] * 100, 1) for r in ["tam", "nuvem+s3_standard|morno", "onprem+onprem|morno"]])
        s += "Granularidade %s, acuracia top-1 estrita / tolerante (%%; tolerante = plano a <= 0,5%% do valor do oraculo). Para os modelos, cada coluna usa o modelo treinado com aquele rotulo:\n\n" % g + tabela(["plano", "oraculo de tamanho", "oraculo economico nuvem+S3 morno", "oraculo economico on-prem morno"], linhas) + "\n"
        linhas = []
        for cl in av["classes"]:
            cel = []
            for nome, key in (("zstd-3 fixo", None), ("probe+zstd-3 (espalhado)", None)):
                cel.append(n(go["linhas_base"][nome]["acuracia"]["tam"]["por_classe"][cl]["estrita"] * 100, 0))
            for m in ("cart", "logit"):
                cel.append(n(go["modelos"][m]["tam"]["lofo"]["acuracia"]["tam"]["por_classe"][cl]["estrita"] * 100, 0))
                cel.append(n(go["modelos"][m]["tam"]["loco"]["acuracia"]["tam"]["por_classe"][cl]["estrita"] * 100, 0))
            linhas.append([cl] + cel)
        s += "Granularidade %s, acuracia estrita (%%) vs oraculo de tamanho, por classe:\n\n" % g + tabela(["classe", "zstd-3 fixo", "probe (espalhado)", "CART LOFO", "CART LOCO", "logistica LOFO", "logistica LOCO"], linhas) + "\n"
    return s


def md_ganhos(av):
    s = ""
    for g, _ in GRANS:
        go = av["granularidades"][g]
        z3 = go["linhas_base"]["zstd-3 fixo"]["resumo"]["global"]
        pr = min((go["linhas_base"][k]["resumo"]["global"] for k in ("probe+zstd-3 (inicio)", "probe+zstd-3 (espalhado)")), key=lambda x: x["bytes"])
        pr_nome = "probe+zstd-3 (inicio)" if pr is go["linhas_base"]["probe+zstd-3 (inicio)"]["resumo"]["global"] else "probe+zstd-3 (espalhado)"
        md = go["linhas_base"]["medir todos"]
        linhas = []
        for m, nm in (("cart", "CART"), ("logit", "logistica"), ("regra", "regra manual")):
            if m == "regra":
                r = go["linhas_base"]["regra manual"]["resumo"]["global"]
                dig = {x: r["valor_usd_gb"][x] for x in ROTULOS_ECO}
            else:
                r = go["modelos"][m]["tam"]["lofo"]["resumo"]["global"]
                dig = {x: go["modelos"][m][x]["lofo"]["resumo"]["global"]["valor_usd_gb"][x] for x in ROTULOS_ECO}
            for ref_nome, ref in (("zstd-3 fixo", z3), (pr_nome, pr)):
                linhas.append([nm, ref_nome, n((ref["bytes"] - r["bytes"]) / ref["bytes"] * 100, 2) + "%"] + [n(dig[x] - ref["valor_usd_gb"][x], 5) for x in ROTULOS_ECO])
            linhas.append([nm, "medir todos", n((md["resumo"]["global"]["bytes"] - r["bytes"]) / md["resumo"]["global"]["bytes"] * 100, 2) + "%"] + [n(dig[x] - md["resumo"]["global"]["valor_usd_gb"][x], 5) for x in ROTULOS_ECO])
        cab = ["modelo", "contra", "ganho de bytes"] + ["US$/GB " + x.replace("nuvem+s3_standard", "nuvem").replace("onprem+onprem", "on-prem") for x in ROTULOS_ECO]
        s += "Granularidade %s (ganho = bytes economizados em relacao a linha de base; US$/GB = valor liquido do modelo menos o da linha de base; modelo em LOFO; as colunas economicas usam o modelo treinado com o rotulo da coluna; a linha de 'medir todos' tem os mesmos bytes do oraculo e desconta o custo de comprimir todos os outros candidatos):\n\n" % g + tabela(cab, linhas)
        t = md["tempo"]
        s += "\nTempo de compressao de 'medir todos' em multiplos do tempo de zstd-3 (vazoes medidas por classe, 1 nucleo): %sx so com os 9 candidatos sincronos, %sx com os 14 candidatos (inclui xz-9e, zstd-19 e bzip2-9).\n\n" % (n(t["tempo_sync_sobre_zstd3"], 1), n(t["tempo_todos_sobre_zstd3"], 0))
    return s


def md_custo(tempo, av, crit):
    S = av["variante"]["amostra_bytes"]
    s = ""
    linhas = []
    for g, _ in GRANS:
        t = tempo["por_gran"][g]["global"]
        usados = tempo["por_gran"][g]["features_usadas_cart"]
        linhas.append([g, t["blocos"], n(t["us_zstd3"], 1), n(t["us_features_full"], 1), n(t["us_features_sub_cart"], 1) + " (" + str(len(usados)) + " feat.)", n(t["us_features_regra"], 1), n(t["us_probe"], 1), n(t["us_inferencia_cart"], 2), n(t["us_inferencia_logit"], 2), n(t["pct_zstd3_sub_mais_cart"], 1) + "%", n(t["pct_zstd3_full_mais_logit"], 1) + "%", n(t["pct_zstd3_regra"], 1) + "%", n(t["pct_zstd3_probe"], 1) + "%"])
    s += tabela(["gran.", "blocos", "zstd-3 (us/bloco)", "features completas (us)", "features do CART (us)", "features da regra (us)", "probe (us)", "inferencia CART (us)", "inferencia logistica (us)", "CART: feat+inf / zstd-3", "logistica: feat+inf / zstd-3", "regra / zstd-3", "probe / zstd-3"], linhas)
    linhas = []
    for g, _ in GRANS:
        t = tempo["por_gran"][g]["global"]
        usados = tempo["por_gran"][g]["features_usadas_cart"]
        linhas.append([g, n(us_nativo(usados, S, t["us_probe"]), 1), n(us_nativo(FEATS, S, t["us_probe"]), 1), n(us_nativo(SUBCONJ_REGRA, S, t["us_probe"]), 1), n(us_nativo(usados, S, t["us_probe"]) / t["us_zstd3"] * 100, 1) + "%", n(us_nativo(FEATS, S, t["us_probe"]) / t["us_zstd3"] * 100, 1) + "%"])
    s += "\nEstimativa para implementacao nativa (INFERENCIA PROPRIA, nao medida): cada feature distinta custa %d passada sobre a amostra a %s B/us (~2 GB/s, histograma escalar), mais o probe zstd-1 medido; passadas do CART: %d, da logistica (21 features): %d, da regra: %d.\n\n" % (1, n(BYTES_POR_US_NATIVO, 0), passos_nativos(tempo["por_gran"]["256K"]["features_usadas_cart"]), passos_nativos(FEATS), passos_nativos(SUBCONJ_REGRA))
    s += tabela(["gran.", "CART nativo (us)", "logistica nativo (us)", "regra nativa (us)", "CART nativo / zstd-3", "logistica nativa / zstd-3"], linhas)
    s += "\nPor classe (granularidade 1M), custo do CART (features do CART + inferencia) sobre o tempo do zstd-3 do mesmo bloco:\n\n"
    linhas = []
    for cl in av["classes"]:
        t = tempo["por_gran"]["1M"]["por_classe"][cl]
        linhas.append([cl, t["blocos"], n(t["us_zstd3"], 1), n(t["us_features_sub_cart"], 1), n(t["pct_zstd3_sub_mais_cart"], 1) + "%"])
    s += tabela(["classe", "blocos", "zstd-3 (us/bloco)", "features do CART (us/bloco)", "CART feat+inf / zstd-3"], linhas)
    return s


def md_criterio(crit):
    s = ""
    linhas = []
    for g, _ in GRANS:
        c = crit[g]
        for m, nm in (("cart", "CART"), ("logit", "logistica"), ("regra", "regra manual")):
            x = c["modelos"][m]
            bs = x["bootstrap_arquivos"]
            linhas.append(
                [
                    g,
                    nm,
                    n(x["i_regret_bytes_pct"], 2) + "% [IC95 " + n(bs["regret_pct_ic95"][0], 2) + "; " + n(bs["regret_pct_ic95"][1], 2) + "] " + ("OK" if x["i_ok"] else "FALHA"),
                    n(x["ii_ganho_vs_probe_pct"], 2) + "% [IC95 " + n(bs["ganho_vs_ref_pct_ic95"][0], 2) + "; " + n(bs["ganho_vs_ref_pct_ic95"][1], 2) + "] " + ("OK" if x["ii_ok"] else "FALHA"),
                    n(x["iii_custo_pct_zstd3_numpy_medido"], 1) + "% " + ("OK" if x["iii_ok_numpy"] else "FALHA") + "; nativo estimado " + n(x["iii_custo_pct_zstd3_nativo_estimado"], 1) + "%",
                    "SIM" if x["todas_ok_medido"] else "NAO",
                ]
            )
    s += tabela(["gran.", "modelo", "(i) regret de bytes <= 1% (LOFO global)", "(ii) ganho >= 1% sobre probe+zstd-3", "(iii) features+inferencia <= 2% do zstd-3", "as 3 pelo medido"], linhas)
    s += "\nProbe+zstd-3 e oraculo de tamanho do caminho sincrono:\n\n"
    linhas = [[g, crit[g]["probe_usado"], n(crit[g]["probe_regret_bytes_pct"], 2) + "%", "sim" if crit[g]["probe_dentro_de_1pct_do_oraculo"] else "nao", crit[g]["melhor_classificador"], crit[g]["veredito"]] for g, _ in GRANS]
    s += tabela(["gran.", "melhor probe (por bytes)", "regret do probe", "probe a <= 1% do oraculo?", "melhor classificador", "veredito"], linhas)
    return s


def md_confusao(av):
    s = ""
    for g, _ in GRANS:
        go = av["granularidades"][g]
        for nome, M in (("CART LOFO", go["confusao"]["cart"]["tam"]), ("probe+zstd-3 (espalhado)", go["confusao"]["probe_espalhado"]["tam"]), ("regra manual", go["confusao"]["regra"]["tam"])):
            linhas = [[CURTO[i]] + [str(v) if v else "." for v in M[i]] for i in range(NS) if sum(M[i])]
            s += "Granularidade %s, matriz de confusao de %s (linhas = plano do oraculo de tamanho; colunas = plano escolhido):\n\n" % (g, nome) + tabela(["oraculo \\ escolhido"] + CURTO, linhas) + "\n"
    return s


def md_erros(av):
    s = ""
    for g, _ in GRANS:
        go = av["granularidades"][g]
        linhas = []
        for e in go["erros_por_arquivo"]["cart"][:8]:
            linhas.append([e["arquivo"], e["blocos"], n(e["bytes_extra"] / 1e3, 1), n(e["regret_pct"], 2) + "%", n(e["acc_estrita"] * 100, 0) + "%"])
        s += "Granularidade %s, os 8 arquivos onde o CART (LOFO) mais gasta bytes alem do oraculo:\n\n" % g + tabela(["arquivo", "blocos", "bytes extra (KB)", "regret", "acerto estrito"], linhas) + "\n"
    return s


def md_transforms(av, tr):
    s = "Vazao dos transforms em numpy (CPU 0, buffer de 1 MiB aleatorio, mediana de 31 repeticoes) e estimativa nativa (INFERENCIA PROPRIA):\n\n"
    linhas = [[k, n(v["apply_MBps"], 0), n(v["invert_MBps"], 0), n(v["nativo_estimado_MBps"], 0)] for k, v in tr.items() if k != "_meta"]
    s += tabela(["transform", "aplicar (MB/s)", "inverter (MB/s)", "estimativa nativa aplicar/inverter (MB/s)"], linhas)
    for g, _ in GRANS:
        go = av["granularidades"][g]["transforms"]["por_classe"]
        linhas = []
        for cl in av["classes"]:
            x = go[cl]
            melhor = max(x["por_transform"], key=lambda k: x["por_transform"][k]["ganho_pct_vs_zstd3"])
            linhas.append([cl, n(x["ganho_pct_vs_zstd3"], 2) + "%", n(x["blocos_transform_vence_3pct"] * 100, 1) + "%", melhor + " " + n(x["por_transform"][melhor]["ganho_pct_vs_zstd3"], 1) + "%", n(x["vazao_zstd3_comp_MBps"], 0) + " -> " + n(x["por_transform"][melhor]["vazao_comp_MBps"], 0), n(x["vazao_zstd3_decomp_MBps"], 0) + " -> " + n(x["por_transform"][melhor]["vazao_decomp_MBps"], 0), (n(x["async_T_xz_ganho_pct_vs_xz"], 1) + "% / " + n(x["async_T_zstd19_ganho_pct_vs_zstd19"], 1) + "% (" + str(x["async_T_blocos"]) + " blocos)") if "async_T_blocos" in x else "-"])
        s += "\nGranularidade %s por classe: ganho do melhor transform ESCOLHIDO POR BLOCO (min entre zstd-3 e os 6 transforms+zstd-3) sobre zstd-3, fracao de blocos em que algum transform vence zstd-3 por >= 3%%, melhor transform fixo para a classe inteira, vazao comp./decomp. em MB/s (numpy) de zstd-3 -> melhor transform fixo+zstd-3, e ganho assincrono de T+xz-9e e T+zstd-19 sobre xz-9e e zstd-19 nos blocos em que T foi tentado:\n\n" % g + tabela(["classe", "ganho por bloco vs zstd-3", "blocos com transform >= 3%", "melhor transform fixo (ganho)", "comp MB/s", "decomp MB/s", "async T+xz / T+z19 vs sem T"], linhas)
    return s


def md_loco(av):
    s = ""
    for g, _ in GRANS:
        go = av["granularidades"][g]
        linhas = []
        for cl in av["classes"]:
            linhas.append([cl] + [n(go["modelos"][m]["tam"][esq]["resumo"]["por_classe"][cl]["regret_bytes_pct"], 2) for m in ("cart", "logit") for esq in ("lofo", "loco")])
        linhas.append(["GLOBAL"] + [n(go["modelos"][m]["tam"][esq]["resumo"]["global"]["regret_bytes_pct"], 2) for m in ("cart", "logit") for esq in ("lofo", "loco")])
        s += "Granularidade %s, regret de bytes (%%) em leave-one-file-out contra leave-one-class-out:\n\n" % g + tabela(["classe", "CART LOFO", "CART LOCO", "logistica LOFO", "logistica LOCO"], linhas) + "\n"
    return s


def md_elegibilidade(av):
    go = av["granularidades"]["1M"]["elegibilidade_sincrona_vazao_comp_MBps"]
    linhas = []
    for cl in av["classes"]:
        linhas.append([cl] + [n(go[cl][c], 0) + ("" if go[cl][c] >= VAZAO_MIN_SINCRONO else " *") for c in SYNC])
    return tabela(["classe"] + CURTO, linhas) + "\n(*) abaixo de 290 MB/s por nucleo na compressao (vazao do codec da classe medida em 1 MiB combinada em serie com o custo do transform em numpy).\n"


def md_modelos(av):
    linhas = []
    for g, _ in GRANS:
        c = av["granularidades"][g]["modelos"]["cart"]["tam"]["tamanho"]
        l = av["granularidades"][g]["modelos"]["logit"]["tam"]["tamanho"]
        linhas.append([g, c["nos"], c["folhas"], ", ".join(c["features_usadas"]), l["pesos"], "x".join(str(v) for v in l["forma"])])
    return tabela(["gran.", "nos do CART", "folhas", "features usadas pelo CART", "pesos da logistica", "forma (21 features + vies) x 9 planos"], linhas)


def md_triagem(av):
    linhas = []
    for t in av["triagem"]:
        linhas.append([t["gran"], t["politica"], t["amostra"], "sim" if t["pesos"] else "nao", n(t["regret_bytes_pct"], 2) + "%", n(t["acc"] * 100, 1) + "%"])
    return tabela(["gran.", "amostragem", "amostra (B)", "pesos por custo", "regret de bytes (CART LOFO)", "acerto estrito"], linhas)


def md_corpus(meta, kman):
    linhas = []
    for a in meta["corpus"]:
        nb = [-(-a["tamanho"] // g) for _, g in GRANS]
        linhas.append([a["nome"], a["classe"], a["tamanho"]] + nb)
    return tabela(["arquivo", "classe", "bytes", "blocos 64K", "blocos 256K", "blocos 1M"], linhas)


def cmd_relatorio(a):
    with open(a.avaliacao) as f:
        av = json.load(f)
    with open(a.tempo) as f:
        tempo = json.load(f)
    with open(a.rotulos_meta) as f:
        meta = json.load(f)
    with open(a.transformes) as f:
        tr = json.load(f)
    nat = None
    if a.avaliacao_nativo:
        with open(a.avaliacao_nativo) as f:
            nat = json.load(f)
    kman = None
    if a.kernel_manifesto and os.path.exists(a.kernel_manifesto):
        with open(a.kernel_manifesto) as f:
            kman = json.load(f)
    crit = criterio_parada(av, tempo)
    sens = None
    if nat:
        sens = {}
        for g, _ in GRANS:
            sens[g] = {}
            for m in ("cart", "logit"):
                sens[g][m] = {r: {"numpy": av["granularidades"][g]["modelos"][m][r]["lofo"]["resumo"]["global"]["regret_usd_gb"][r], "nativo": nat["granularidades"][g]["modelos"][m][r]["lofo"]["resumo"]["global"]["regret_usd_gb"][r]} for r in ROTULOS_ECO}
            sens[g]["zstd-3 fixo"] = {r: {"numpy": av["granularidades"][g]["linhas_base"]["zstd-3 fixo"]["resumo"]["global"]["regret_usd_gb"][r], "nativo": nat["granularidades"][g]["linhas_base"]["zstd-3 fixo"]["resumo"]["global"]["regret_usd_gb"][r]} for r in ROTULOS_ECO}
    saida = {
        "meta": {
            "data": time.strftime("%Y-%m-%d %H:%M:%S"),
            "python": sys.version.split()[0],
            "numpy": np.__version__,
            "cpu": platform.processor() or platform.machine(),
            "candidatos": CANDS,
            "granularidades_bytes": {g: v for g, v in GRANS},
            "limiar_transform_async": meta["limiar_transform"],
            "blocos": meta["blocos"],
            "comparacoes_bit_exatas": meta["comparacoes_bit_exatas"],
            "nao_exatos": meta["nao_exatos"],
            "falhas_reconstrucao": meta["falhas"],
            "corpus": meta["corpus"],
            "kernel": kman,
        },
        "transformes": tr,
        "avaliacao_custo_transform_numpy": av,
        "sensibilidade_custo_transform_nativo": sens,
        "tempo": tempo,
        "criterio_parada": crit,
    }
    with open(a.json, "w") as f:
        json.dump(saida, f)
    L = []
    L.append("# Plano por bloco: um classificador barato escolhe transform + codec + nivel?\n")
    L.append("Gerado por `python3 -I bench/plano_bloco.py relatorio` em %s. Numeros brutos em `plano_bloco.json` (por granularidade: resumo por plano e por classe, acuracias, matrizes de confusao, modelos finais com pesos/nos portaveis, tempos) e rotulos por bloco em `plano_bloco_rotulos.npz`. Metodologia, definicoes e limitacoes no `bench/README.md` (secao Rodada 3).\n" % time.strftime("%Y-%m-%d %H:%M"))
    L.append("## 1. Veredito (criterio de parada)\n")
    for g, _ in GRANS:
        L.append("- **%s**: %s (melhor classificador: %s)." % (g, crit[g]["veredito"], crit[g]["melhor_classificador"]))
    L.append("\nDetalhe por condicao (regret e ganho em leave-one-file-out agrupado; IC95 = bootstrap sobre os arquivos, 2000 reamostragens):\n")
    L.append(md_criterio(crit))
    L.append("\n## 2. Dados e bit-exatidao\n")
    ne = meta["nao_exatos"]
    L.append("Corpus: %d arquivos/grupos, %s bytes. Blocos independentes: %s. Comparacoes (bloco, candidato) com decodificacao e sha256 do bloco: %s. **Falhas de reconstrucao: %d** (blocos nao exatos por granularidade: %s); a reversibilidade `invert(apply(x)) == x` dos transforms foi verificada em todo bloco e tambem contra uma implementacao de referencia ingenua da semantica de `src/transform.rs` em 846 casos de borda (`autoteste`).\n" % (len(meta["corpus"]), n(sum(a["tamanho"] for a in meta["corpus"]), 0), json.dumps(meta["blocos"]), json.dumps(meta["comparacoes_bit_exatas"]), len(meta["falhas"]), json.dumps(ne)))
    if kman:
        L.append("Classe `codigo_kernel`: primeiros %d bytes do fluxo tar de `%s` (streaming com `lzma`, %s bytes de xz baixados, %s bytes de conteudo de %d arquivos regulares agrupados em %d pseudo-arquivos por diretorio; o sha256 do tar.xz inteiro NAO foi conferido porque so o inicio foi baixado).\n" % (kman["bytes_tar_consumidos"], kman["url"], n(kman["bytes_xz_baixados"], 0), n(sum(g["bytes"] for g in kman["grupos"].values()), 0), kman["arquivos_regulares_lidos"], len(kman["grupos"])))
    L.append(md_corpus(meta, kman))
    L.append("\n## 3. Oraculo\n")
    L.append("### 3.1 Distribuicao do oraculo de tamanho (caminho sincrono)\n")
    L.append(md_oraculo(av))
    L.append("### 3.2 Caminho assincrono\n")
    L.append(md_async(av))
    L.append("\n## 4. Triagem de amostragem, tamanho da amostra e pesos\n")
    L.append("Variante escolhida: **%s, amostra de %d B, pesos por custo = %s** (%s).\n" % (av["variante"]["politica"], av["variante"]["amostra_bytes"], "sim" if av["pesos_escolhidos"] else "nao", av["criterio_escolha_variante"]))
    L.append(md_triagem(av))
    L.append("\n## 5. Modelos (tamanho)\n")
    L.append(md_modelos(av))
    L.append("\n## 6. Acuracia top-1\n")
    L.append(md_acuracia(av))
    L.append("\n## 7. Regret global por plano, granularidade e camada\n")
    L.append(md_regret_global(av))
    L.append("\n## 8. Regret por classe\n")
    L.append(md_regret_classe(av))
    L.append("\n## 9. Ganho do classificador\n")
    L.append(md_ganhos(av))
    L.append("\n## 10. Custo de features e inferencia\n")
    L.append(md_custo(tempo, av, crit))
    L.append("\n## 11. Onde o modelo erra\n")
    L.append(md_erros(av))
    L.append(md_confusao(av))
    L.append("\n## 12. Generalizacao a classes novas (leave-one-class-out)\n")
    L.append(md_loco(av))
    L.append("\n## 13. Transforms\n")
    L.append(md_transforms(av, tr))
    L.append("\n## 14. Elegibilidade ao caminho sincrono (>= 290 MB/s por nucleo na compressao, granularidade 1 MiB)\n")
    L.append(md_elegibilidade(av))
    if sens:
        L.append("\n## 15. Sensibilidade ao custo do transform (numpy medido contra nativo estimado), regret em US$/GB\n")
        linhas = []
        for g, _ in GRANS:
            for m in ("zstd-3 fixo", "cart", "logit"):
                linhas.append([g, m] + [n(sens[g][m][r]["numpy"], 5) + " / " + n(sens[g][m][r]["nativo"], 5) for r in ROTULOS_ECO])
        L.append(tabela(["gran.", "plano"] + ["numpy / nativo " + r.replace("nuvem+s3_standard", "nuvem").replace("onprem+onprem", "on-prem") for r in ROTULOS_ECO], linhas))
    L.append("\nConferencia interna: diferenca maxima absoluta entre o valor liquido vetorizado deste programa e `economia.Contexto.liquido_gb` em 200 pares (bloco, candidato) sorteados por granularidade: %s US$/GB.\n" % ("%.2e" % av["verificacao_valor_vs_economia_py_max_abs_usd_gb"]))
    with open(a.md, "w") as f:
        f.write("\n".join(L))
    print("relatorio gravado:", a.json, a.md)


def cmd_autoteste(a):
    r = autoteste_transforms()
    print(json.dumps(r))
    sys.exit(0 if r["passou"] else 1)


def main():
    ap = argparse.ArgumentParser()
    sp = ap.add_subparsers(dest="cmd", required=True)
    p = sp.add_parser("autoteste")
    p.set_defaults(f=cmd_autoteste)
    p = sp.add_parser("kernel")
    p.add_argument("--url", default=URL_KERNEL)
    p.add_argument("--destino", required=True)
    p.add_argument("--limite-mib", type=int, default=256)
    p.set_defaults(f=cmd_kernel)
    p = sp.add_parser("rotular")
    p.add_argument("--dados", required=True)
    p.add_argument("--kernel", default=None)
    p.add_argument("--saida", required=True)
    p.add_argument("--processos", type=int, default=6)
    p.add_argument("--limiar-transform", type=float, default=0.03)
    p.set_defaults(f=cmd_rotular)
    p = sp.add_parser("features")
    p.add_argument("--dados", required=True)
    p.add_argument("--kernel", default=None)
    p.add_argument("--saida", required=True)
    p.add_argument("--processos", type=int, default=6)
    p.set_defaults(f=cmd_features)
    p = sp.add_parser("custo-transform")
    p.add_argument("--saida", required=True)
    p.set_defaults(f=cmd_custo_transform)
    p = sp.add_parser("avaliar")
    p.add_argument("--rotulos", required=True)
    p.add_argument("--features", required=True)
    p.add_argument("--medicoes", default=os.path.join(BASE, "resultados", "medicoes.json"))
    p.add_argument("--precos", default=os.path.join(BASE, "precos.json"))
    p.add_argument("--transformes", required=True)
    p.add_argument("--custo-transform", choices=["numpy", "nativo"], default="numpy")
    p.add_argument("--saida", required=True)
    p.set_defaults(f=cmd_avaliar)
    p = sp.add_parser("tempo")
    p.add_argument("--dados", required=True)
    p.add_argument("--kernel", default=None)
    p.add_argument("--avaliacao", required=True)
    p.add_argument("--saida", required=True)
    p.set_defaults(f=cmd_tempo)
    p = sp.add_parser("relatorio")
    p.add_argument("--avaliacao", required=True)
    p.add_argument("--avaliacao-nativo", default=None)
    p.add_argument("--tempo", required=True)
    p.add_argument("--rotulos-meta", required=True)
    p.add_argument("--transformes", required=True)
    p.add_argument("--kernel-manifesto", default=None)
    p.add_argument("--json", required=True)
    p.add_argument("--md", required=True)
    p.set_defaults(f=cmd_relatorio)
    a = ap.parse_args()
    a.f(a)


if __name__ == "__main__":
    main()
