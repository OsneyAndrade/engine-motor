import ast
import contextlib
import io
import json
import math
import os
import re
import resource
import runpy
import stat
import sys
import time
import tokenize

import numpy as np

BASE = os.path.dirname(os.path.abspath(__file__))
ARQUIVOS_FONTE = ["perfil_dataset.py", "perfil_testes.py", "perfil_sintetico.py", "perfil_validar.py"]
MODULOS_PROIBIDOS = {"socket", "ssl", "urllib", "urllib3", "http", "ftplib", "smtplib", "poplib", "imaplib", "telnetlib", "xmlrpc", "requests", "aiohttp", "asyncio", "subprocess", "pty", "ctypes", "cffi", "webbrowser"}
CHAMADAS_PROIBIDAS = {"system", "popen", "execv", "execve", "execvp", "execl", "execlp", "spawnl", "spawnv", "posix_spawn", "eval", "exec", "__import__"}
EVENTOS_PROIBIDOS = ("socket.", "subprocess.", "os.system", "os.exec", "os.posix_spawn", "os.spawn", "ctypes.")
EVENTOS_ESCRITA = ("os.mkdir", "os.rename", "os.remove", "os.rmdir", "os.symlink", "os.link", "os.chmod", "os.truncate", "os.chown", "os.utime", "os.mkfifo", "os.mknod", "shutil.", "os.unlink")


class Auditoria:
    def __init__(self):
        self.ativo = False
        self.eventos = []
        sys.addaudithook(self.gancho)

    def gancho(self, evento, args):
        if not self.ativo:
            return
        if evento == "open" or evento.startswith(EVENTOS_ESCRITA) or evento.startswith(EVENTOS_PROIBIDOS):
            self.eventos.append((evento, args))

    @contextlib.contextmanager
    def janela(self):
        self.eventos = []
        self.ativo = True
        try:
            yield self
        finally:
            self.ativo = False


class Falha(AssertionError):
    pass


def exige(cond, msg):
    if not cond:
        raise Falha(msg)


def como_str(p):
    if isinstance(p, int):
        return None
    if isinstance(p, bytes):
        return os.fsdecode(p)
    return str(p)


def arvore_estado(raiz):
    saida = {}
    for d, ds, fs in os.walk(os.fsencode(raiz)):
        ds.sort()
        for f in sorted(fs):
            p = os.path.join(d, f)
            st = os.lstat(p)
            h = None
            if stat.S_ISREG(st.st_mode) and (st.st_mode & 0o444):
                import hashlib

                with open(p, "rb") as fh:
                    h = hashlib.sha256(fh.read()).hexdigest()
            saida[p] = (st.st_size, st.st_mtime_ns, stat.S_IMODE(st.st_mode), h)
    return saida


class Contexto:
    def __init__(self, tool, base):
        self.tool = tool
        self.base = base
        self.sint = tool.carregar_modulo("perfil_sintetico", os.path.join(BASE, "perfil_sintetico.py"))
        self._manifesto = None
        self._runs = {}
        self.aud = Auditoria()
        self.n_runs = 0

    def manifesto(self):
        if self._manifesto is None:
            self._manifesto = self.sint.construir(os.path.join(self.base, "ds"))
        return self._manifesto

    def rodar(self, chave, revelar=False, processos=2, extra=(), raiz=None, cmd="tudo", auditar=False, semente=1, saida=None):
        if chave in self._runs and not auditar:
            return self._runs[chave]
        self.n_runs += 1
        raiz = raiz or self.manifesto()["raiz"]
        saida = saida or os.path.join(self.base, "saida_%s" % chave)
        argv = [cmd, "--raiz", raiz, "--saida", saida, "--processos", str(processos), "--semente", str(semente), "--amostra-mib", "3", "--amostra-mib-lento", "1", "--reamostras", "200", "--max-pares", "40", "--delta-mib", "4"] + list(extra)
        if revelar:
            argv.append("--revelar-nomes")
        buf = io.StringIO()
        err = io.StringIO()
        t0 = time.time()
        if auditar:
            with self.aud.janela():
                with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(err):
                    rc = self.tool.main(argv)
        else:
            with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(err):
                rc = self.tool.main(argv)
        r = {"rc": rc, "saida": saida, "stdout": buf.getvalue(), "stderr": err.getvalue(), "tempo": time.time() - t0, "raiz": raiz}
        if rc == 0 and os.path.exists(os.path.join(saida, "perfil.json")):
            with open(os.path.join(saida, "perfil.json")) as f:
                r["perfil"] = json.load(f)
            with open(os.path.join(saida, "registro_leitura_tudo.jsonl")) as f:
                r["registro"] = [json.loads(l) for l in f]
        if not auditar:
            self._runs[chave] = r
        return r


def amostras_classificacao(tool, sint):
    rng = np.random.default_rng(11)
    voc = sint.vocabulario(rng)
    return rng, voc


def cls(tool, dados, nome):
    ext = tool.extensao(nome.encode())
    head = dados[: tool.JANELA_CLASSIFICACAO]
    am = head
    if len(dados) > tool.MIB:
        j = tool.JANELA_CLASSIFICACAO
        am = head + dados[(len(dados) - j) // 2 : (len(dados) - j) // 2 + j] + dados[-j:]
    return tool.classificar_amostra(head, am, ext, len(dados))[0]


def t_classifica_texto(c):
    rng, voc = amostras_classificacao(c.tool, c.sint)
    d = c.sint.texto_sintetico(rng, 60000, voc)
    exige(cls(c.tool, d, "notas.txt") == "texto", "texto .txt")
    exige(cls(c.tool, d, "semextensao") == "texto", "texto sem extensao")
    exige(cls(c.tool, d.decode().encode("utf-16"), "u16.txt") == "texto", "texto utf-16 com BOM")


def t_classifica_codigo(c):
    fonte = open(os.path.join(BASE, "syntrabench.py"), "rb").read()
    exige(cls(c.tool, fonte, "x.py") == "codigo", "codigo por extensao")
    exige(cls(c.tool, fonte, "semextensao") == "codigo", "codigo python sem extensao por conteudo")
    csrc = b'#include <stdio.h>\n#define N 4\nstatic int f(int a) {\n  if (a > N) { return a; }\n  for (int i = 0; i < a; i++) { a += i; }\n  return a;\n}\n' * 40
    exige(cls(c.tool, csrc, "dados") == "codigo", "codigo C sem extensao por conteudo")
    exige(cls(c.tool, b"#!/bin/sh\necho oi\n" * 30, "run") == "codigo", "shebang")


def t_classifica_csv_json_log_xml_dna(c):
    rng, voc = amostras_classificacao(c.tool, c.sint)
    exige(cls(c.tool, c.sint.csv_sintetico(rng, 3000, 1), "tabela.dat") == "csv_tabular", "csv por conteudo")
    exige(cls(c.tool, c.sint.csv_sintetico(rng, 3000, 1), "t.csv") == "csv_tabular", "csv por extensao")
    nd = c.sint.ndjson_sintetico(rng, 800)
    exige(cls(c.tool, nd, "ev.dat") == "json_ndjson", "ndjson por conteudo")
    exige(cls(c.tool, b'{"a": [1, 2, 3], "b": {"c": "d"}}', "x.dat") == "json_ndjson", "json pequeno")
    exige(cls(c.tool, c.sint.log_sintetico(rng, 800, 3), "app.txt") == "log", "log por conteudo")
    exige(cls(c.tool, b'<?xml version="1.0"?>\n<a><b>1</b><b>2</b></a>\n' * 50, "doc") == "xml_html", "xml")
    exige(cls(c.tool, b"<!DOCTYPE html><html><body><p>oi</p></body></html>" * 20, "p.html") == "xml_html", "html")
    seq = b">chr1\n" + b"".join(bytes(rng.choice(list(b"ACGT"), 70).astype(np.uint8)) + b"\n" for _ in range(300))
    exige(cls(c.tool, seq, "g.dat") == "sequencia", "fasta")


def t_magic_containers(c):
    rng = np.random.default_rng(3)
    corpo = rng.bytes(40000)
    for nome, mag in (("a.dat", b"PK\x03\x04"), ("b.dat", b"\x1f\x8b\x08\x00"), ("c.dat", b"\xfd7zXZ\x00"), ("d.dat", b"\x28\xb5\x2f\xfd"), ("e.dat", b"BZh91AY&SY"), ("f.dat", b"7z\xbc\xaf\x27\x1c"), ("g.dat", b"PAR1"), ("h.dat", b"ORC"), ("i.dat", b"\x04\x22\x4d\x18")):
        exige(cls(c.tool, mag + corpo, nome) == "container_comprimido", "magic de container " + nome)


def t_magic_midia(c):
    rng = np.random.default_rng(4)
    corpo = rng.bytes(40000)
    casos = [
        ("a.dat", b"\x89PNG\r\n\x1a\n"),
        ("b.dat", b"\xff\xd8\xff\xe0\x00\x10JFIF"),
        ("c.dat", b"GIF89a"),
        ("d.dat", b"ID3\x03\x00"),
        ("e.dat", b"\x00\x00\x00\x20ftypisom"),
        ("f.dat", b"RIFF\x00\x00\x00\x00WEBPVP8 "),
        ("g.dat", b"OggS\x00\x02"),
        ("h.dat", b"fLaC\x00\x00"),
        ("i.dat", b"\x1a\x45\xdf\xa3"),
    ]
    for nome, mag in casos:
        exige(cls(c.tool, mag + corpo, nome) == "imagem_audio_video", "magic de midia " + nome)


def t_magic_binarios_e_banco(c):
    rng = np.random.default_rng(5)
    corpo = rng.integers(0, 40, 40000, dtype=np.uint8).tobytes()
    exige(cls(c.tool, b"\x7fELF\x02\x01\x01" + corpo, "prog") == "binario", "ELF")
    exige(cls(c.tool, b"SQLite format 3\x00" + corpo, "dados") == "banco_de_dados", "sqlite")
    exige(cls(c.tool, b"\x89HDF\r\n\x1a\n" + corpo, "dados") == "cientifico_numerico", "hdf5")
    exige(cls(c.tool, b"Salted__" + rng.bytes(5000), "dados") == "criptografado_aleatorio", "openssl enc")
    pe = bytearray(b"MZ" + bytes(62)) + b"PE\x00\x00" + corpo
    pe[60:64] = (64).to_bytes(4, "little")
    exige(cls(c.tool, bytes(pe), "x.dll") == "binario", "PE")


def t_aleatorio_sem_magic(c):
    rng = np.random.default_rng(6)
    exige(cls(c.tool, rng.bytes(200000), "dados.bin") == "criptografado_aleatorio", "aleatorio 200 KB")
    exige(cls(c.tool, rng.bytes(2048), "dados.bin") == "criptografado_aleatorio", "aleatorio 2 KB (correcao de vies da entropia)")
    exige(cls(c.tool, rng.bytes(3 * (1 << 20)), "dados.bin") == "criptografado_aleatorio", "aleatorio 3 MiB com tres janelas")
    exige(cls(c.tool, rng.bytes(60000), "foto.jpg") == "imagem_audio_video", "extensao de midia sem assinatura")


def t_numerico_entropia_deltas(c):
    rng = np.random.default_rng(7)
    exige(cls(c.tool, c.sint.serie_int16(rng, 60000), "leitura.dat") == "cientifico_numerico", "int16 suave com ruido")
    rampa = (np.arange(30000) * 3 + rng.integers(0, 5, 30000)).astype("<i4").tobytes()
    exige(cls(c.tool, rampa, "x.dat") == "cientifico_numerico", "rampa int32")
    sem_magic = open("/usr/bin/env", "rb").read()[64:] if os.path.exists("/usr/bin/env") else rng.integers(0, 30, 50000, dtype=np.uint8).tobytes()
    exige(cls(c.tool, sem_magic[:200000], "x.dat") == "binario", "executavel sem cabecalho nao e numerico")


def t_vazio(c):
    exige(c.tool.classificar_amostra(b"", b"", "txt", 0)[0] == "vazio", "tamanho 0")


def t_inventario_percentis(c):
    import tempfile

    with tempfile.TemporaryDirectory(dir=c.base) as t:
        raiz = os.path.join(t, "r")
        os.makedirs(raiz)
        tam = [10, 100, 1000, 10000, 100000, 0]
        for i, n in enumerate(tam):
            with open(os.path.join(raiz, "f%d.txt" % i), "wb") as f:
                f.write(b"a" * n)
        r = c.rodar("percentis", raiz=raiz, cmd="varrer", processos=1)
        exige(r["rc"] == 0, "varrer falhou: " + r["stderr"])
        with open(os.path.join(r["saida"], "varredura.json")) as f:
            v = json.load(f)
        inv = v["inventario"]
        exige(inv["n_arquivos"] == 6 and inv["bytes"] == sum(tam), "contagem e bytes")
        ts = inv["tamanho_dos_arquivos"]
        exige(ts["max"] == 100000 and abs(ts["p50"] - np.percentile(sorted(tam), 50)) < 1e-6, "percentis de tamanho")
        exige(inv["por_classe"]["vazio"]["arquivos"] == 1, "arquivo vazio contado")
        exige(inv["por_extensao_top25_bytes"][0]["extensao"] == "txt", "extensoes")


def t_symlink_nao_seguido(c):
    m = c.manifesto()
    r = c.rodar("auditada", auditar=True, processos=1)
    exige(r["rc"] == 0, "execucao falhou: " + r["stderr"][-300:])
    inv = r["perfil"]["inventario"]
    exige(inv["ignorados_por_motivo"].get("link_simbolico") == len(m["links"]), "links ignorados: %s" % inv["ignorados_por_motivo"])
    for e, args in c.aud.eventos:
        if e == "open":
            p = como_str(args[0]) or ""
            exige("SEGREDO_EXTERNO" not in p and "link_para_fora" not in p and "link_arquivo_fora" not in p, "abriu algo atras de link: " + p)
    for item in r["registro"]:
        exige(item["status"] != "lido" or item["classe"] is not None, "registro incoerente")
    com_marcador = False
    for d, ds, fs in os.walk(r["saida"]):
        for f in fs:
            if m["marcador_externo"] in open(os.path.join(d, f), "rb").read():
                com_marcador = True
    exige(not com_marcador, "conteudo externo apareceu na saida")


def t_somente_leitura_e_nofollow(c):
    m = c.manifesto()
    raiz = os.path.realpath(m["raiz"])
    antes = arvore_estado(raiz)
    r = c.rodar("auditada2", auditar=True, processos=1)
    exige(r["rc"] == 0, "execucao falhou")
    depois = arvore_estado(raiz)
    exige(antes == depois, "a raiz mudou (conteudo, tamanho, mtime ou modo)")
    saida = os.path.realpath(r["saida"])
    n_ro = 0
    for e, args in c.aud.eventos:
        if e != "open":
            continue
        p = como_str(args[0])
        flags = args[2] if len(args) > 2 and isinstance(args[2], int) else 0
        if p is None:
            continue
        escrita = flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC | os.O_APPEND)
        if not os.path.isabs(p):
            n_ro += 1
            exige(not escrita, "abertura com escrita na raiz: " + p)
            exige(flags & os.O_NOFOLLOW, "abertura sem O_NOFOLLOW: " + p)
        elif os.path.realpath(p).startswith(raiz + os.sep):
            raise Falha("abertura por caminho absoluto dentro da raiz: " + p)
        elif escrita:
            exige(os.path.realpath(p).startswith(saida + os.sep) or os.path.realpath(p) == saida, "escrita fora de --saida: " + p)
    exige(n_ro > 50, "poucas aberturas observadas (%d)" % n_ro)


def t_nao_escreve_fora_da_saida(c):
    r = c.rodar("auditada3", auditar=True, processos=1)
    saida = os.path.realpath(r["saida"])
    raiz = os.path.realpath(r["raiz"])
    n = 0
    for e, args in c.aud.eventos:
        if e == "open":
            flags = args[2] if len(args) > 2 and isinstance(args[2], int) else 0
            if not flags & (os.O_WRONLY | os.O_RDWR | os.O_CREAT | os.O_TRUNC | os.O_APPEND):
                continue
            p = como_str(args[0])
        elif e.startswith(EVENTOS_ESCRITA):
            p = como_str(args[0]) if args and not isinstance(args[0], int) else None
        else:
            continue
        if p is None:
            continue
        n += 1
        rp = os.path.realpath(p)
        exige(rp == saida or rp.startswith(saida + os.sep), "escrita fora de --saida: %s (%s)" % (e, p))
        exige(not rp.startswith(raiz + os.sep), "escrita dentro da raiz: " + p)
    exige(n > 5, "poucas escritas observadas")
    exige(not any(e.startswith(EVENTOS_PROIBIDOS) for e, _ in c.aud.eventos), "evento de rede ou execucao: %s" % [e for e, _ in c.aud.eventos if e.startswith(EVENTOS_PROIBIDOS)][:3])
    exige(os.stat(r["saida"]).st_mode & 0o077 == 0, "--saida criada com permissao aberta")


def t_saida_dentro_da_raiz_recusada(c):
    m = c.manifesto()
    buf = io.StringIO()
    with contextlib.redirect_stderr(buf):
        rc = c.tool.main(["varrer", "--raiz", m["raiz"], "--saida", os.path.join(m["raiz"], "v1", "saida")])
    exige(rc == 2 and "dentro de --raiz" in buf.getvalue(), "deveria recusar --saida dentro da raiz")
    exige(not os.path.exists(os.path.join(m["raiz"], "v1", "saida", "varredura.json")), "escreveu mesmo assim")


def t_sem_imports_de_rede_nem_execucao(c):
    for nome in ARQUIVOS_FONTE:
        caminho = os.path.join(BASE, nome)
        if not os.path.exists(caminho):
            continue
        arvore = ast.parse(open(caminho).read())
        for no in ast.walk(arvore):
            if isinstance(no, ast.Import):
                for a in no.names:
                    exige(a.name.split(".")[0] not in MODULOS_PROIBIDOS, "%s importa %s" % (nome, a.name))
            elif isinstance(no, ast.ImportFrom):
                exige((no.module or "").split(".")[0] not in MODULOS_PROIBIDOS, "%s importa de %s" % (nome, no.module))
            elif isinstance(no, ast.Call):
                f = no.func
                n = f.attr if isinstance(f, ast.Attribute) else (f.id if isinstance(f, ast.Name) else None)
                if n in CHAMADAS_PROIBIDAS and nome != "perfil_testes.py":
                    raise Falha("%s chama %s" % (nome, n))


def t_sem_comentarios_no_codigo(c):
    for nome in ARQUIVOS_FONTE:
        caminho = os.path.join(BASE, nome)
        if not os.path.exists(caminho):
            continue
        with open(caminho, "rb") as f:
            for tok in tokenize.tokenize(f.readline):
                if tok.type == tokenize.COMMENT:
                    exige(tok.start[0] == 1 and tok.string.startswith("#!"), "%s:%d comentario" % (nome, tok.start[0]))


def t_executa_nada_do_dado(c):
    import tempfile

    with tempfile.TemporaryDirectory(dir=c.base) as t:
        raiz = os.path.join(t, "r")
        os.makedirs(raiz)
        marca = os.path.join(t, "EXECUTOU")
        with open(os.path.join(raiz, "arma.sh"), "w") as f:
            f.write("#!/bin/sh\ntouch %s\n" % marca)
        os.chmod(os.path.join(raiz, "arma.sh"), 0o755)
        with open(os.path.join(raiz, "arma.py"), "w") as f:
            f.write("import os\nopen(%r, 'w').write('x')\n" % marca)
        with open(os.path.join(raiz, "json.py"), "w") as f:
            f.write("open(%r, 'w').write('x')\n" % marca)
        with open(os.path.join(raiz, "; touch EXECUTOU"), "w") as f:
            f.write("x" * 100)
        r = c.rodar("naoexec", raiz=raiz, auditar=True, processos=1)
        exige(r["rc"] == 0, "falhou: " + r["stderr"][-200:])
        exige(not os.path.exists(marca), "algo do dado foi executado")
        exige(not any(e.startswith(EVENTOS_PROIBIDOS) for e, _ in c.aud.eventos), "evento de execucao")


def varrer_tokens(c, nomes_dir):
    todos = set()
    for d, ds, fs in os.walk(os.fsencode(nomes_dir)):
        for n in ds + fs:
            todos.add(n)
    return todos


def corpo_da_saida(saida, so_texto=False):
    import zipfile

    partes = []
    for d, ds, fs in os.walk(saida):
        for f in fs:
            p = os.path.join(d, f)
            if so_texto and f.endswith(".npz"):
                continue
            b = open(p, "rb").read()
            partes.append(b)
            if f.endswith(".npz"):
                with zipfile.ZipFile(p) as z:
                    for n in z.namelist():
                        partes.append(n.encode())
                        partes.append(z.read(n))
        for n in ds + fs:
            partes.append(n.encode())
    return b"\n".join(partes)


def candidatos_de_nomes(c, m):
    candidatos = set()
    for nome in varrer_tokens(c, m["raiz"]):
        s = nome.decode("utf-8", "surrogateescape")
        candidatos.add(s)
        candidatos.add(os.path.splitext(s)[0])
        for t in re.split(r"[^\w]+", s):
            if len(t) >= 5:
                candidatos.add(t)
    candidatos |= set(m["nomes_sensiveis"])
    candidatos |= {os.path.basename(m["raiz"]), os.path.basename(m["base"]), "externo", "SEGREDO_EXTERNO"}
    return candidatos


def vazamentos_em(c, saida, candidatos, base_txt):
    vocabulario_fixo = list(c.tool.CLASSES) + list(c.tool.CODECS) + [v[0] for v in c.tool.PROXY_MEDICOES.values()] + list(c.tool.MOTIVOS_ERRNO.values())
    vocabulario_fixo += ["outro_dispositivo", "profundidade_maxima", "hardlink_repetido", "erro_stat", "alterado_durante_leitura", "substituido_durante_leitura", "nao_regular", "ilegivel"]
    permitidas = [x.lower() for x in vocabulario_fixo] + ["bin", "dat", "raw", "txt", "csv", "log", "ndjson", "sqlite", "xz", "gz", "zip", "jpg", "mp4", "parquet", "py"]
    corpo_l = corpo_da_saida(saida).lower()
    achados = []
    for tok in sorted(candidatos):
        if len(tok) < 4:
            continue
        b = tok.encode("utf-8", "surrogateescape").lower()
        if b in base_txt or any(tok.lower() in x for x in permitidas):
            continue
        if b in corpo_l:
            achados.append(tok)
    return achados


def t_anonimizacao_sem_vazamento(c):
    m = c.manifesto()
    r = c.rodar("anon")
    exige(r["rc"] == 0, "falhou: " + r["stderr"][-300:])
    vazio = os.path.join(c.base, "vazio_ref")
    os.makedirs(vazio, exist_ok=True)
    ref = c.rodar("ref_vazio", raiz=vazio, cmd="tudo", processos=1)
    base_txt = corpo_da_saida(ref["saida"]).lower()
    candidatos = candidatos_de_nomes(c, m)
    vazamentos = vazamentos_em(c, r["saida"], candidatos, base_txt)
    exige(not vazamentos, "nomes vazaram na saida anonimizada: %s" % vazamentos[:8])
    corpo = corpo_da_saida(r["saida"])
    corpo_texto = corpo_da_saida(r["saida"], so_texto=True)
    exige(b"\xff\xfe" not in corpo_texto and b"\xc3(" not in corpo_texto, "bytes invalidos de nome na saida")
    exige(m["raiz"].encode() not in corpo, "caminho da raiz na saida")
    exige(r["perfil"]["meta"]["anonimizado"] is True, "meta nao marca anonimizado")
    controle = c.rodar("revelar", revelar=True)
    achados = vazamentos_em(c, controle["saida"], candidatos, base_txt)
    exige(len(achados) >= 10, "o detector de vazamento nao achou os nomes na saida com --revelar-nomes (%d): detector fraco" % len(achados))


def t_revelar_nomes_mostra_nomes(c):
    r = c.rodar("revelar", revelar=True)
    exige(r["rc"] == 0, "falhou")
    corpo = corpo_da_saida(r["saida"])
    exige(b"capitulos_1" in corpo, "nomes deveriam aparecer com --revelar-nomes")
    exige("relat\u00f3rio final".encode() in corpo or b"relat\\u00f3rio final" in corpo, "nome com acento deveria aparecer com --revelar-nomes")
    exige(r["perfil"]["meta"]["anonimizado"] is False, "meta deveria marcar nao anonimizado")


def t_registro_de_leitura(c):
    m = c.manifesto()
    r = c.rodar("anon")
    reg = r["perfil"]["registro_leitura"]
    inv = r["perfil"]["inventario"]
    linhas = r["registro"]
    n_ign = sum(inv["ignorados_por_motivo"].values())
    exige(len(linhas) == inv["n_arquivos"] + n_ign, "linhas do registro %d != inventario %d + ignorados %d" % (len(linhas), inv["n_arquivos"], n_ign))
    soma = sum(sum(x["bytes_lidos"].values()) for x in linhas)
    exige(soma == reg["bytes_lidos_total"], "bytes lidos do registro nao fecham")
    exige(reg["ilegiveis_por_motivo"].get("sem_permissao_leitura") == 1, "ilegivel nao registrado: %s" % reg["ilegiveis_por_motivo"])
    exige(inv["ignorados_por_motivo"].get("sem_permissao_diretorio") == 1, "diretorio trancado nao registrado")
    exige(inv["ignorados_por_motivo"].get("nao_regular") == 1, "fifo nao registrado")
    exige(all("caminho" not in x for x in linhas), "registro anonimo com caminho")
    exige(reg["nomes_com_bytes_invalidos_em_utf8"] >= 1, "nomes invalidos nao contados")
    exige(0 < reg["fracao_lida_do_inventario"] <= 4.0, "fracao lida implausivel: %s" % reg["fracao_lida_do_inventario"])


def t_ilegivel_nao_aborta(c):
    r = c.rodar("anon")
    exige(r["rc"] == 0, "deveria terminar")
    il = [x for x in r["registro"] if x["status"] == "sem_permissao_leitura"]
    exige(len(il) == 1 and il[0]["classe"] == "ilegivel", "arquivo sem permissao deveria constar como ilegivel")
    exige(os.getuid() != 0, "teste de permissao nao vale como root")


def t_nomes_invalidos_utf8(c):
    r = c.rodar("revelar", revelar=True)
    exige(r["rc"] == 0, "falhou com nomes invalidos")
    caminhos = [x["caminho"] for x in r["registro"] if "caminho" in x]
    exige(any("inv\\xff\\xfe_SEGREDO" in p for p in caminhos), "nome com byte invalido nao processado")
    exige(any("dir_\\xc3(_inv" in p for p in caminhos), "diretorio com byte invalido nao processado")
    for nome in os.listdir(r["saida"]):
        if nome.endswith(".json"):
            json.load(open(os.path.join(r["saida"], nome)))
        elif nome.endswith(".jsonl"):
            for l in open(os.path.join(r["saida"], nome)):
                json.loads(l)


def sem_ids(o):
    if isinstance(o, dict):
        return {k: sem_ids(v) for k, v in o.items() if k not in ("id", "data", "tempo_por_fase_s", "vazao_comp_indicativa_MBps", "ic95_liquido_usd_mes")}
    if isinstance(o, list):
        return [sem_ids(x) for x in o]
    return o


def t_determinismo_por_semente(c):
    a = c.rodar("det_a", processos=1, semente=7)
    b = c.rodar("det_b", processos=3, semente=7)
    d = c.rodar("det_c", processos=2, semente=8)
    exige(a["rc"] == b["rc"] == d["rc"] == 0, "falhou")
    pa = sem_ids(a["perfil"]["compressibilidade"]["por_classe"])
    pb = sem_ids(b["perfil"]["compressibilidade"]["por_classe"])
    pd = sem_ids(d["perfil"]["compressibilidade"]["por_classe"])
    exige(pa == pb, "mesma semente deveria dar o mesmo resultado (1 e 3 processos)")
    exige(sem_ids(a["perfil"]["familias"]) == sem_ids(b["perfil"]["familias"]), "familias/delta nao deterministicos")
    exige(pa != pd, "sementes diferentes deveriam sortear blocos diferentes")
    ids_a = {g["id"] for g in a["perfil"]["dedup"]["maiores_grupos"]}
    ids_b = {g["id"] for g in b["perfil"]["dedup"]["maiores_grupos"]}
    exige(ids_a.isdisjoint(ids_b), "o sal deveria ser aleatorio entre execucoes (ids de grupo iguais)")


def t_dedup_copias_e_mesmo_tamanho(c):
    m = c.manifesto()
    r = c.rodar("revelar", revelar=True)
    grupos = r["perfil"]["dedup"]["maiores_grupos"]
    todos = r["perfil"]["dedup"]
    exige(todos["grupos_de_duplicatas"] >= 4, "poucos grupos")
    juntos = [set(g["caminhos"]) for g in grupos if "caminhos" in g]
    for grp in m["duplicatas_exatas_dentro_de_v1"]:
        exige(any(set(grp) <= j for j in juntos), "grupo de copias nao encontrado: %s" % grp[0])
    for a, b in m["mesmo_tamanho_diferentes"]:
        exige(not any(a in j and b in j for j in juntos), "arquivos diferentes de mesmo tamanho foram agrupados: %s %s" % (a, b))
    exige(todos["bytes_lidos_hash_completo"] > 0, "hash completo nao foi usado nos candidatos")
    n_dup_esperado = sum(len(g) - 1 for g in m["duplicatas_exatas_dentro_de_v1"]) + len(m["pares_identicos_entre_versoes"])
    exige(todos["arquivos_redundantes"] >= n_dup_esperado, "redundantes %d < esperado %d" % (todos["arquivos_redundantes"], n_dup_esperado))


def t_dedup_hash_completo_igual(c):
    a = c.rodar("revelar", revelar=True)
    b = c.rodar("hc", revelar=True, extra=["--hash-completo"])
    exige(b["rc"] == 0, "falhou com --hash-completo")
    da = a["perfil"]["dedup"]
    db = b["perfil"]["dedup"]
    exige(da["bytes_duplicados"] == db["bytes_duplicados"] and da["grupos_de_duplicatas"] == db["grupos_de_duplicatas"], "--hash-completo mudou o resultado")
    exige(db["bytes_lidos_hash_completo"] >= da["bytes_lidos_hash_completo"], "hash completo deveria ler pelo menos o mesmo")


def t_familias_v1_v2(c):
    m = c.manifesto()
    r = c.rodar("revelar", revelar=True)
    fam = r["perfil"]["familias"]
    exige(fam["n_familias"] >= len(m["pares_versoes_modificadas"]) * 0.8, "poucas familias: %d" % fam["n_familias"])
    exige(fam["pares_com_falha_de_roundtrip"] == 0, "falha de roundtrip no delta")
    ag = fam["delta_agregado_todas_as_classes"]
    exige(ag is not None and ag["razao_patch_sobre_z3"] < 0.5, "delta de versoes com 10%% editado deveria custar bem menos que zstd-3 isolado: %s" % (ag and ag["razao_patch_sobre_z3"]))
    exige(0.0 < fam["pct_bytes_em_familias"] < 1.0, "fracao em familias implausivel")
    exige(r["perfil"]["resumo_executivo"][3].startswith("Familias de versoes:"), "resumo sem familias")


def t_familias_acham_pares_conhecidos(c):
    import tempfile

    with tempfile.TemporaryDirectory(dir=c.base) as t:
        raiz = os.path.join(t, "r")
        rng = np.random.default_rng(1)
        base = c.sint.texto_sintetico(rng, 400000, c.sint.vocabulario(rng))
        for d, nome, dados in (("v1/dados", "tabela.txt", base), ("v2/dados", "tabela.txt", c.sint.editar(rng, base, 0.1)), ("snap_2026-11-01", "export_v1.txt", base[:300000]), ("snap_2026-11-02", "export_v2.txt", c.sint.editar(rng, base[:300000], 0.05))):
            os.makedirs(os.path.join(raiz, d), exist_ok=True)
            with open(os.path.join(raiz, d, nome), "wb") as f:
                f.write(dados)
        r = c.rodar("fam_conhecidas", raiz=raiz, cmd="varrer", processos=1, revelar=True)
        with open(os.path.join(r["saida"], "varredura.json")) as f:
            v = json.load(f)
        exige(v["familias"]["n_familias"] == 2, "esperava 2 familias, achou %d" % v["familias"]["n_familias"])
        exige(v["familias"]["pares_medidos"] == 2, "esperava 2 pares medidos")
        exige(v["familias"]["delta_agregado_todas_as_classes"]["razao_patch_sobre_z3"] < 0.4, "delta baixo esperado")


def t_nao_confunde_nomes_parecidos(c):
    import tempfile

    with tempfile.TemporaryDirectory(dir=c.base) as t:
        raiz = os.path.join(t, "r")
        os.makedirs(raiz)
        rng = np.random.default_rng(2)
        voc = c.sint.vocabulario(rng)
        for nome in ("capitulo_1.txt", "capitulo_2.txt", "capitulo_3.txt"):
            with open(os.path.join(raiz, nome), "wb") as f:
                f.write(c.sint.texto_sintetico(rng, 100000, voc))
        r = c.rodar("fam_irmaos", raiz=raiz, cmd="varrer", processos=1)
        with open(os.path.join(r["saida"], "varredura.json")) as f:
            v = json.load(f)
        exige(v["familias"]["n_familias"] == 0, "arquivos numerados como irmaos nao sao versoes")


def carregar_modelo_cliente():
    caminho = os.path.join(BASE, "..", "docs", "anexos", "modelo_cliente.py")
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        return runpy.run_path(caminho)


def t_conta_bate_modelo_cliente(c):
    t = c.tool
    mc = carregar_modelo_cliente()
    P, _ = t.carregar_params(None)
    P["volume_mensal_bytes"]["valor"] = mc["P_GB"] * 1e9
    P["retencao_meses"]["valor"] = mc["T"]
    info = {"bytes": 1, "share": 1.0, "dup_frac": 0.0, "fam_frac": 0.0}
    vaz = {"zstd-3": (mc["COMP_MBS"], mc["DEC_MBS"])}
    s = {"zstd-3": mc["S"]}
    n = 0
    for ret in (0.8, 0.9, 0.95):
        for nome, (p, cpu_h) in mc["ARMAZ"].items():
            for c_move, m, n_dec in ((0.0, 1, 1), (0.01, 3, 3), (0.09, 1, 10)):
                Q = {k: dict(v) for k, v in P.items()}
                Q["fracao_retida"]["valor"] = ret
                Q["preco_armazenamento_gb_mes"]["valor"] = p
                Q["preco_nucleo_hora"]["valor"] = cpu_h
                Q["custo_movimento_usd_por_gb"]["valor"] = c_move
                Q["passagens_movimento"]["valor"] = m
                Q["leituras_por_gb_processado"]["valor"] = n_dec
                ref = mc["linha"](ret, p, c_move, m, n_dec, cpu_h)
                r = t.avaliar_classe(Q, info, {"sync": "zstd-3"}, vaz, s, None, False, False)
                for a, b in ((ref[1], r["armazenamento_usd_mes"]), (ref[0], r["armazenamento_usd_mes"]), (ref[2], r["movimento_usd_mes"]), (ref[3], r["cpu_usd_mes"])):
                    exige(abs(a - b) <= 1e-9 * max(1.0, abs(a)), "divergiu do modelo_cliente: %r vs %r" % (a, b))
                n += 1
    exige(n == 27, "grade incompleta")
    exige(abs(t.cpu_gb(mc["CPU_H"], mc["COMP_MBS"], 1.0) - mc["custo_cpu_gb"](mc["COMP_MBS"])) < 1e-15, "custo de CPU por GB")


def t_conta_economica_fim_a_fim_bate_modelo_cliente(c):
    t = c.tool
    mc = carregar_modelo_cliente()
    B = 50
    opc = t.Opc(reamostras=B)
    est = t.Estado(opc)
    est.boot[("texto", "_n")] = B
    for cod in t.CODECS:
        est.boot[("texto", cod)] = np.full(B, 1.0 - mc["S"])
    est.delta_vetores = {}
    codecs = {cod: {"economia": mc["S"], "ic95_economia": [mc["S"], mc["S"]]} for cod in t.CODECS}
    amo = {"por_classe": {"texto": {"bytes_populacao": 1000, "codecs": codecs, "baixa_confianca": False}}}
    var = {
        "por_classe_resumo": {"texto": {"bytes": 1000, "bytes_duplicados": 0, "bytes_nao_base_em_familias": 0}},
        "familias": {"delta_por_classe": {}, "delta_agregado_todas_as_classes": None},
        "inventario": {"por_classe": {"texto": {"bytes": 1000}}},
        "meta": {"opcoes": {"max_pares": 1, "delta_mib": 1}},
    }
    P, avisos = t.carregar_params(None)
    P["volume_mensal_bytes"]["valor"] = mc["P_GB"] * 1e9
    P["retencao_meses"]["valor"] = mc["T"]
    P["fracao_retida"]["valor"] = 0.9
    P["custo_movimento_usd_por_gb"]["valor"] = 0.05
    P["passagens_movimento"]["valor"] = 3
    P["leituras_por_gb_processado"]["valor"] = 3
    P["idade_recompressao_meses"]["valor"] = mc["T"]
    P["dedup_aplicavel"]["valor"] = False
    P["delta_aplicavel"]["valor"] = False
    P["vazao_mbps"]["valor"] = {"texto": {cod: {"comp": mc["COMP_MBS"], "decomp": mc["DEC_MBS"]} for cod in ("zstd-3", "zstd-19", "xz-9e")}}
    eco = t.estimar_economia(est, var, amo, P, avisos, {})
    ref = mc["linha"](0.9, 0.023, 0.05, 3, 3, mc["CPU_H"])
    tot = eco["total"]["politica_recomendada_com_dedup_e_delta"]
    exige(abs(tot["armazenamento_usd_mes"]["estimativa"] - ref[1]) < 1e-6 * ref[1], "armazenamento fim a fim")
    exige(abs(tot["movimento_usd_mes"]["estimativa"] - ref[2]) < 1e-6 * ref[2], "movimento fim a fim")
    exige(abs(tot["cpu_usd_mes"]["estimativa"] - ref[3]) < 1e-6 * ref[3], "cpu fim a fim")
    exige(abs(tot["liquido_usd_mes"]["estimativa"] - (ref[1] + ref[2] - ref[3])) < 1e-6 * ref[1], "liquido fim a fim")
    exige(eco["por_classe"]["texto"]["politica_sincrona"] == "zstd-3" and eco["por_classe"]["texto"]["politica_assincrona"] is None, "politica")


def t_politica_por_classe(c):
    r = c.rodar("anon")
    eco = r["perfil"]["estimativa_economica"]["por_classe"]
    exige(eco["criptografado_aleatorio"]["politica_sincrona"] == "stored", "aleatorio deveria ser stored")
    exige(eco["imagem_audio_video"]["politica_sincrona"] == "stored", "midia deveria ser stored")
    exige(eco["texto"]["politica_sincrona"] == "zstd-3", "texto deveria usar zstd-3")
    exige(eco["texto"]["liquido_usd_mes"] > 0 and eco["criptografado_aleatorio"]["liquido_usd_mes"] <= eco["texto"]["liquido_usd_mes"], "liquido")
    for classe, d in eco.items():
        for p in ("vazoes_usadas",):
            exige(all("origem" in v for v in d[p].values()), "vazao sem rotulo de origem")


def t_config_fonte_e_tipo(c):
    import tempfile

    t = c.tool
    with tempfile.TemporaryDirectory(dir=c.base) as d:
        ok = os.path.join(d, "ok.json")
        with open(ok, "w") as f:
            json.dump({"_nota": "x", "preco_armazenamento_gb_mes": {"valor": 0.01, "unidade": "USD/GB-mes", "fonte": "contrato do cliente", "tipo": "verificado"}, "leituras_por_gb_processado": 7}, f)
        P, av = t.carregar_params(ok)
        exige(P["preco_armazenamento_gb_mes"]["valor"] == 0.01 and P["preco_armazenamento_gb_mes"]["fonte"] == "contrato do cliente", "valor, fonte e tipo preservados")
        exige(P["leituras_por_gb_processado"]["tipo"] == "nao_informado" and len(av) >= 1, "valor puro deveria gerar aviso")
        for conteudo in ({"tipo_inexistente": 1}, {"preco_nucleo_hora": {"valor": 1, "fonte": "x", "tipo": "chute"}}, {"fracao_retida": {"valor": 1.5, "fonte": "x", "tipo": "premissa"}}):
            ruim = os.path.join(d, "ruim.json")
            with open(ruim, "w") as f:
                json.dump(conteudo, f)
            try:
                t.carregar_params(ruim)
            except t.ErroUso:
                continue
            raise Falha("config invalido aceito: %s" % conteudo)
    for k, p in t.PARAMS_PADRAO.items():
        exige(p["tipo"] in t.TIPOS_PARAM and p["fonte"], "parametro padrao sem fonte/tipo: " + k)


def t_limite_de_processos(c):
    exige(c.tool.Opc(processos=99).processos <= 5, "mais de 5 trabalhadores")
    exige(c.tool.Opc(processos=0).processos == 1, "minimo de 1")


def t_ic_e_estimador_contra_o_censo(c):
    m = c.manifesto()
    t = c.tool
    cen = c.rodar("amostrado", revelar=False)
    exige(cen["rc"] == 0, "falhou")
    sub = os.path.join(c.base, "saida_censo2")
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        rc = t.main(["tudo", "--raiz", m["raiz"], "--saida", sub, "--processos", "2", "--amostra-mib", "100000", "--amostra-mib-lento", "100000", "--reamostras", "200", "--max-pares", "3", "--delta-mib", "1"])
    exige(rc == 0, "censo falhou")
    verd = json.load(open(os.path.join(sub, "amostra.json")))["por_classe"]
    amo = cen["perfil"]["compressibilidade"]["por_classe"]
    cobre = 0
    total = 0
    for classe, d in amo.items():
        if d["arquivos_populacao"] < 5 or d["fracao_amostrada"] >= 0.999:
            continue
        v = verd[classe]["codecs"]["zstd-3"]["economia"]
        e = d["codecs"]["zstd-3"]
        total += 1
        erro = abs(e["economia"] - v)
        exige(erro < 0.03, "erro de %.3f em %s" % (erro, classe))
        if e["ic95_economia"][0] - 1e-9 <= v <= e["ic95_economia"][1] + 1e-9:
            cobre += 1
    exige(total >= 3 and cobre >= max(1, int(0.6 * total)), "IC95 cobriu o censo em %d de %d classes" % (cobre, total))
    for classe, d in verd.items():
        exige(d["censo"], "classe %s nao foi censo" % classe)


def t_fracao_incompressivel(c):
    import tempfile

    with tempfile.TemporaryDirectory(dir=c.base) as t:
        raiz = os.path.join(t, "r")
        os.makedirs(raiz)
        rng = np.random.default_rng(9)
        voc = c.sint.vocabulario(rng)
        for k in range(6):
            with open(os.path.join(raiz, "t%d.txt" % k), "wb") as f:
                f.write(c.sint.texto_sintetico(rng, 1 << 20, voc))
            with open(os.path.join(raiz, "r%d.bin" % k), "wb") as f:
                f.write(rng.bytes(1 << 20))
        r = c.rodar("incomp", raiz=raiz, processos=2)
        inv = r["perfil"]["inventario"]
        exige(abs(inv["fracao_incompressivel_por_classificacao"] - 0.5) < 0.01, "por classificacao: %s" % inv["fracao_incompressivel_por_classificacao"])
        inc = r["perfil"]["compressibilidade"]["dataset"]["incompressivel_por_amostra"]
        exige(abs(inc["fracao_dos_bytes"] - 0.5) < 0.01, "por amostra: %s" % inc["fracao_dos_bytes"])


def t_ganho_de_transform_em_cientifico(c):
    r = c.rodar("anon")
    amo = r["perfil"]["compressibilidade"]["por_classe"]
    exige("cientifico_numerico" in amo, "sem classe cientifica")
    tr = amo["cientifico_numerico"].get("transform")
    exige(tr is not None and tr["ganho_pp_sobre_zstd3"] >= 0.0, "ganho de transform ausente ou negativo")
    exige(tr["economia_melhor_por_bloco"] >= amo["cientifico_numerico"]["codecs"]["zstd-3"]["economia"] - 1e-12, "melhor por bloco nao pode perder para zstd-3 puro")
    exige(abs(sum(tr["escolha_pct_dos_blocos"].values()) - 1.0) < 1e-9, "escolhas nao somam 1")
    exige(all("transform" not in d for k, d in amo.items() if k != "cientifico_numerico"), "transform so para cientifico")


def t_hardlink_e_limite_de_arquivos(c):
    import tempfile

    with tempfile.TemporaryDirectory(dir=c.base) as t:
        raiz = os.path.join(t, "r")
        os.makedirs(raiz)
        for k in range(10):
            with open(os.path.join(raiz, "f%02d.txt" % k), "wb") as f:
                f.write(b"abc %d\n" % k * 100)
        os.link(os.path.join(raiz, "f00.txt"), os.path.join(raiz, "hl.txt"))
        r = c.rodar("hl", raiz=raiz, cmd="varrer", processos=1)
        v = json.load(open(os.path.join(r["saida"], "varredura.json")))
        exige(v["inventario"]["n_arquivos"] == 10 and v["inventario"]["ignorados_por_motivo"].get("hardlink_repetido") == 1, "hardlink deveria contar uma vez")
        r2 = c.rodar("trunc", raiz=raiz, cmd="varrer", processos=1, extra=["--max-arquivos", "4"])
        v2 = json.load(open(os.path.join(r2["saida"], "varredura.json")))
        exige(v2["inventario"]["n_arquivos"] == 4 and v2["inventario"]["inventario_truncado_por_max_arquivos"], "--max-arquivos")


def t_nunca_carrega_arquivo_inteiro(c):
    import tempfile

    with tempfile.TemporaryDirectory(dir=c.base) as t:
        raiz = os.path.join(t, "r")
        os.makedirs(raiz)
        for nome in ("grande_a.dat", "grande_b.dat"):
            fd = os.open(os.path.join(raiz, nome), os.O_WRONLY | os.O_CREAT, 0o644)
            os.ftruncate(fd, 1 << 30)
            os.close(fd)
        antes = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        r = c.rodar("grande", raiz=raiz, processos=1, extra=["--amostra-mib", "4", "--amostra-mib-lento", "1"])
        exige(r["rc"] == 0, "falhou")
        depois = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        exige((depois - antes) < 400 * 1024, "pico de memoria cresceu %d KiB" % (depois - antes))
        reg = r["perfil"]["registro_leitura"]
        dd = r["perfil"]["dedup"]
        exige(reg["bytes_lidos_por_fase"]["amostra_compressibilidade"] <= 8 << 20, "amostrou demais: %d" % reg["bytes_lidos_por_fase"]["amostra_compressibilidade"])
        exige(reg["bytes_lidos_por_fase"]["classificacao"] <= 2 * 3 * 16384 + 1, "classificou lendo demais")
        exige(dd["bytes_lidos_hash_amostra"] <= 2 * 3 * 65536, "hash de amostra leu demais")
        exige(dd["grupos_de_duplicatas"] == 1 and dd["bytes_duplicados"] == 1 << 30, "os dois arquivos esparsos sao identicos")


def t_subcomandos_separados(c):
    import tempfile

    m = c.manifesto()
    saida = os.path.join(c.base, "saida_sep")
    comuns = ["--raiz", m["raiz"], "--saida", saida, "--processos", "2", "--semente", "1", "--amostra-mib", "3", "--amostra-mib-lento", "1", "--reamostras", "200", "--max-pares", "40", "--delta-mib", "4"]
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        exige(c.tool.main(["varrer"] + comuns) == 0, "varrer")
        exige(c.tool.main(["amostrar"] + comuns) == 0, "amostrar")
        exige(c.tool.main(["estimar", "--saida", saida]) == 0, "estimar")
        exige(c.tool.main(["relatorio", "--saida", saida]) == 0, "relatorio")
    sep = json.load(open(os.path.join(saida, "perfil.json")))
    tudo = c.rodar("anon")["perfil"]
    a = sep["estimativa_economica"]["total"]["politica_recomendada_com_dedup_e_delta"]["liquido_usd_mes"]
    b = tudo["estimativa_economica"]["total"]["politica_recomendada_com_dedup_e_delta"]["liquido_usd_mes"]
    exige(abs(a["estimativa"] - b["estimativa"]) < 1e-6 * abs(b["estimativa"]), "estimar separado difere do tudo")
    exige(a["ic95"] == b["ic95"], "IC separado difere do tudo")
    exige(os.path.exists(os.path.join(saida, "perfil.md")), "perfil.md")


def t_relatorio_formato(c):
    r = c.rodar("anon")
    p = r["perfil"]
    exige(len(p["resumo_executivo"]) == 10, "resumo executivo deve ter 10 linhas")
    md = open(os.path.join(r["saida"], "perfil.md")).read()
    for sec in ("## Resumo executivo", "## 1. Inventario", "## 2. Classificacao", "## 3. Dedup", "## 4. Familias", "## 5. Compressibilidade", "## 6. Estimativa economica", "## 7. O que e medido", "## 8. Registro de leitura", "## 9. Limites e o que nao sabemos"):
        exige(sec in md, "falta a secao " + sec)
    for k in ("medido_no_dataset", "premissa", "desconhecido"):
        exige(p["estimativa_economica"]["origem_dos_numeros"][k], "falta " + k)
    for classe, d in p["estimativa_economica"]["por_classe"].items():
        exige(d["politica"] and d["ic95_liquido_usd_mes"][0] is not None, "classe sem politica ou IC")
    exige(json.dumps(p, allow_nan=False), "NaN no JSON")


def t_aviso_saida_antiga_com_nomes(c):
    import tempfile

    with tempfile.TemporaryDirectory(dir=c.base) as t:
        raiz = os.path.join(t, "r")
        os.makedirs(raiz)
        with open(os.path.join(raiz, "SEGREDO_arquivo.txt"), "wb") as f:
            f.write(b"conteudo " * 500)
        saida = os.path.join(t, "s")
        r1 = c.rodar("antiga1", raiz=raiz, saida=saida, revelar=True, processos=1)
        exige(r1["rc"] == 0 and "aviso" not in r1["stderr"], "primeira execucao nao deveria avisar")
        r2 = c.rodar("antiga2", raiz=raiz, saida=saida, cmd="varrer", processos=1)
        exige(r2["rc"] == 0 and "aviso" in r2["stderr"] and "perfil.json" in r2["stderr"], "deveria avisar de saida anterior com nomes: %r" % r2["stderr"])
        exige("SEGREDO" not in r2["stderr"], "o aviso nao pode citar nomes do dataset")


def t_classes_poucos_arquivos_baixa_confianca(c):
    r = c.rodar("anon")
    amo = r["perfil"]["compressibilidade"]["por_classe"]
    exige(amo["banco_de_dados"]["baixa_confianca"] is True, "banco com 2 arquivos deveria ser baixa confianca")
    exige(amo["banco_de_dados"]["ic95_por"] == "bloco", "poucos arquivos: IC por bloco")
    exige(amo["texto"]["baixa_confianca"] is False and amo["texto"]["ic95_por"] == "arquivo", "texto tem arquivos suficientes")


TESTES = [
    ("classifica_texto", t_classifica_texto),
    ("classifica_codigo", t_classifica_codigo),
    ("classifica_csv_json_log_xml_dna", t_classifica_csv_json_log_xml_dna),
    ("magic_bytes_containers_comprimidos", t_magic_containers),
    ("magic_bytes_imagem_audio_video", t_magic_midia),
    ("magic_bytes_binarios_banco_cientifico_cripto", t_magic_binarios_e_banco),
    ("aleatorio_sem_magic_por_entropia", t_aleatorio_sem_magic),
    ("numerico_por_entropia_dos_deltas", t_numerico_entropia_deltas),
    ("arquivo_vazio", t_vazio),
    ("inventario_percentis_e_extensoes", t_inventario_percentis),
    ("nao_segue_symlink_para_fora_da_raiz", t_symlink_nao_seguido),
    ("raiz_intacta_e_abertura_somente_leitura_nofollow", t_somente_leitura_e_nofollow),
    ("nao_escreve_fora_da_saida_e_sem_rede_ou_execucao", t_nao_escreve_fora_da_saida),
    ("saida_dentro_da_raiz_recusada", t_saida_dentro_da_raiz_recusada),
    ("codigo_sem_imports_de_rede_nem_execucao", t_sem_imports_de_rede_nem_execucao),
    ("codigo_sem_comentarios", t_sem_comentarios_no_codigo),
    ("nada_do_dado_e_executado", t_executa_nada_do_dado),
    ("anonimizacao_nenhum_nome_vaza", t_anonimizacao_sem_vazamento),
    ("revelar_nomes_mostra_nomes", t_revelar_nomes_mostra_nomes),
    ("registro_de_leitura_fecha_com_o_inventario", t_registro_de_leitura),
    ("arquivo_ilegivel_nao_aborta", t_ilegivel_nao_aborta),
    ("nomes_com_bytes_invalidos_em_utf8", t_nomes_invalidos_utf8),
    ("determinismo_por_semente_e_sal_aleatorio", t_determinismo_por_semente),
    ("dedup_acha_copias_e_nao_confunde_mesmo_tamanho", t_dedup_copias_e_mesmo_tamanho),
    ("dedup_hash_completo_igual_ao_prefiltro", t_dedup_hash_completo_igual),
    ("familias_acham_v1_v2_e_delta_barato", t_familias_v1_v2),
    ("familias_pares_conhecidos", t_familias_acham_pares_conhecidos),
    ("arquivos_numerados_nao_viram_familia", t_nao_confunde_nomes_parecidos),
    ("conta_bate_modelo_cliente_grade_27_casos", t_conta_bate_modelo_cliente),
    ("conta_fim_a_fim_bate_modelo_cliente", t_conta_economica_fim_a_fim_bate_modelo_cliente),
    ("politica_por_classe", t_politica_por_classe),
    ("config_exige_fonte_e_tipo", t_config_fonte_e_tipo),
    ("limite_de_processos", t_limite_de_processos),
    ("estimador_e_ic_contra_o_censo", t_ic_e_estimador_contra_o_censo),
    ("fracao_incompressivel", t_fracao_incompressivel),
    ("ganho_de_transform_so_em_cientifico", t_ganho_de_transform_em_cientifico),
    ("hardlink_conta_uma_vez_e_max_arquivos", t_hardlink_e_limite_de_arquivos),
    ("nunca_carrega_arquivo_inteiro", t_nunca_carrega_arquivo_inteiro),
    ("subcomandos_separados_igual_a_tudo", t_subcomandos_separados),
    ("relatorio_formato_e_secoes", t_relatorio_formato),
    ("aviso_de_saida_anterior_com_nomes", t_aviso_saida_antiga_com_nomes),
    ("poucos_arquivos_baixa_confianca", t_classes_poucos_arquivos_baixa_confianca),
]


def seguro(e):
    return str(e).encode("utf-8", "backslashreplace").decode("utf-8")


def executar(tool, filtro=""):
    import shutil
    import tempfile

    base = tempfile.mkdtemp(prefix="perfil-autoteste-")
    ctx = Contexto(tool, base)
    passou = 0
    falhas = []
    t0 = time.time()
    try:
        for nome, fn in TESTES:
            if filtro and filtro not in nome:
                continue
            t1 = time.time()
            try:
                fn(ctx)
                passou += 1
                print("[ok]    %-58s %5.1fs" % (nome, time.time() - t1), flush=True)
            except AssertionError as e:
                falhas.append((nome, seguro(e)))
                print("[FALHA] %-58s %s" % (nome, seguro(e)), flush=True)
            except Exception as e:
                falhas.append((nome, "%s: %s" % (type(e).__name__, seguro(e))))
                print("[ERRO]  %-58s %s: %s" % (nome, type(e).__name__, seguro(e)), flush=True)
    finally:
        ctx.sint.destruir(base)
        shutil.rmtree(base, ignore_errors=True)
    print("autoteste: %d testes, %d passaram, %d falharam, %.0fs" % (passou + len(falhas), passou, len(falhas), time.time() - t0))
    return 0 if not falhas else 1
