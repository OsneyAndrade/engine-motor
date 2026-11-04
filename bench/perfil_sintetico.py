import gzip
import lzma
import os
import shutil
import sqlite3
import stat
import zipfile

import numpy as np

MIB = 1 << 20
KIB = 1 << 10
SEMENTE_PADRAO = 20261104
MARCADOR_EXTERNO = b"MARCADOR-EXTERNO-7c1f9a52-NAO-DEVE-SER-LIDO"
NOMES_SENSIVEIS = [
    "CONFIDENCIAL",
    "cliente_acme",
    "relatório final ação",
    "dir com espaço",
    "ação #1",
    "SEGREDO",
    "vendas_2026",
    "dicionario",
    "dickens",
    "cofre",
]


def vocabulario(rng, n=3000):
    letras = np.array(list("abcdefghijklmnopqrstuvwxyz"))
    comp = rng.integers(2, 11, n)
    return ["".join(letras[rng.integers(0, 26, int(c))]) for c in comp]


def texto_sintetico(rng, n_bytes, vocab):
    pesos = 1.0 / np.arange(1, len(vocab) + 1)
    pesos /= pesos.sum()
    saida = []
    total = 0
    while total < n_bytes:
        idx = rng.choice(len(vocab), size=4000, p=pesos)
        linhas = []
        for k in range(0, 4000, 12):
            linhas.append(" ".join(vocab[i] for i in idx[k : k + 12]).capitalize() + ".")
        bloco = "\n".join(linhas) + "\n"
        saida.append(bloco)
        total += len(bloco)
    return "".join(saida).encode()[:n_bytes]


def csv_sintetico(rng, linhas, mes):
    cats = ["alfa", "beta", "gama", "delta", "epsilon", "zeta", "eta", "teta", "iota", "kappa"]
    ids = np.arange(linhas) + mes * 1000000
    ts = 1767225600 + mes * 2592000 + np.cumsum(rng.integers(1, 90, linhas))
    val = np.round(rng.normal(250, 60, linhas), 2)
    cat = rng.integers(0, len(cats), linhas)
    qtd = rng.integers(1, 40, linhas)
    out = ["id,timestamp,valor,categoria,quantidade,regiao"]
    for i in range(linhas):
        out.append("%d,%d,%.2f,%s,%d,R%d" % (ids[i], ts[i], val[i], cats[cat[i]], qtd[i], cat[i] % 4))
    return ("\n".join(out) + "\n").encode()


def log_sintetico(rng, linhas, dia):
    niveis = ["INFO", "INFO", "INFO", "DEBUG", "WARN", "ERROR"]
    msgs = ["requisicao concluida", "cache atualizado", "conexao aberta", "tempo limite excedido", "usuario autenticado", "fila vazia", "gravando lote", "falha ao ler campo"]
    seg = np.cumsum(rng.integers(0, 3, linhas))
    lv = rng.integers(0, len(niveis), linhas)
    ms = rng.integers(0, len(msgs), linhas)
    ip = rng.integers(1, 255, (linhas, 2))
    out = []
    for i in range(linhas):
        s = int(seg[i])
        out.append("2026-11-%02d %02d:%02d:%02d,%03d %s [worker-%d] %s from 10.0.%d.%d id=%08x" % (dia, (s // 3600) % 24, (s // 60) % 60, s % 60, (i * 7) % 1000, niveis[lv[i]], i % 8, msgs[ms[i]], ip[i, 0], ip[i, 1], int(rng.integers(0, 2**32))))
    return ("\n".join(out) + "\n").encode()


def ndjson_sintetico(rng, linhas):
    evs = ["click", "view", "compra", "saida"]
    out = []
    for i in range(linhas):
        out.append('{"ts": %d, "usuario": %d, "evento": "%s", "valor": %.3f, "ok": %s}' % (1767225600 + i * 3, int(rng.integers(1, 5000)), evs[int(rng.integers(0, 4))], float(rng.random() * 100), "true" if rng.random() > 0.1 else "false"))
    return ("\n".join(out) + "\n").encode()


def serie_float(rng, n):
    return np.cumsum(rng.normal(0, 1, n)).astype("<f8").tobytes()


def serie_int16(rng, n):
    t = np.arange(n)
    return (2000 + 800 * np.sin(t / 37.0) + rng.normal(0, 25, n)).astype("<i2").tobytes()


def criar_sqlite(caminho, rng, linhas):
    if os.path.exists(caminho):
        os.unlink(caminho)
    con = sqlite3.connect(caminho)
    con.execute("create table pedidos (id integer primary key, cliente text, valor real, nota text)")
    voc = vocabulario(rng, 500)
    rows = [(i, "cli%05d" % int(rng.integers(0, 3000)), float(rng.normal(100, 30)), " ".join(voc[int(j)] for j in rng.integers(0, 500, 8))) for i in range(linhas)]
    con.executemany("insert into pedidos values (?,?,?,?)", rows)
    con.commit()
    con.close()


def atualizar_sqlite(origem, destino, rng, linhas):
    shutil.copyfile(origem, destino)
    con = sqlite3.connect(destino)
    voc = vocabulario(rng, 500)
    n = con.execute("select count(*) from pedidos").fetchone()[0]
    for _ in range(max(1, n // 20)):
        i = int(rng.integers(0, n))
        con.execute("update pedidos set nota=?, valor=? where id=?", (" ".join(voc[int(j)] for j in rng.integers(0, 500, 8)), float(rng.normal(100, 30)), i))
    rows = [(n + i, "cli%05d" % int(rng.integers(0, 3000)), float(rng.normal(100, 30)), "novo " + voc[int(rng.integers(0, 500))]) for i in range(linhas)]
    con.executemany("insert into pedidos values (?,?,?,?)", rows)
    con.commit()
    con.close()


def editar(rng, dados, fracao=0.10, fonte=None):
    a = bytearray(dados)
    n = len(a)
    if n < 8192:
        return bytes(a)
    origem = fonte if fonte is not None and len(fonte) > 8192 else dados
    m = len(origem)
    alvo = int(n * fracao)
    feito = 0
    while feito < alvo:
        L = int(rng.integers(256, 4096))
        p = int(rng.integers(0, n - L))
        q = int(rng.integers(0, max(1, m - L)))
        a[p : p + L] = origem[q : q + L]
        feito += L
    for _ in range(5):
        L = int(rng.integers(50, 2000))
        p = int(rng.integers(0, len(a)))
        q = int(rng.integers(0, max(1, m - L)))
        a[p:p] = origem[q : q + L]
    return bytes(a)


def gravar(base, rel, dados, modo=0o644):
    caminho = os.path.join(base, rel) if isinstance(rel, str) else os.path.join(os.fsencode(base), rel)
    os.makedirs(os.path.dirname(caminho), exist_ok=True)
    with open(caminho, "wb") as f:
        f.write(dados)
    os.chmod(caminho, modo)
    return caminho


def ler(caminho, n=None):
    with open(caminho, "rb") as f:
        return f.read() if n is None else f.read(n)


def py_stdlib(qtd, minimo, pular=0):
    raiz = os.path.dirname(os.__file__)
    achados = []
    for d, _, fs in os.walk(raiz):
        if "site-packages" in d or "test" in d or "__pycache__" in d:
            continue
        for f in fs:
            if f.endswith(".py"):
                p = os.path.join(d, f)
                try:
                    if os.path.getsize(p) >= minimo:
                        achados.append(p)
                except OSError:
                    pass
    achados.sort()
    return achados[pular : pular + qtd]


def elfs(qtd):
    achados = []
    for nome in sorted(os.listdir("/usr/bin")):
        p = os.path.join("/usr/bin", nome)
        try:
            if os.path.isfile(p) and not os.path.islink(p) and 120 * KIB <= os.path.getsize(p) <= 3 * MIB and ler(p, 4) == b"\x7fELF":
                achados.append(p)
        except OSError:
            pass
    return achados[:qtd]


def construir(base, semente=SEMENTE_PADRAO, silesia=None, canterbury=None, real=False):
    rng = np.random.default_rng(semente)
    raiz = os.path.join(base, "raiz")
    externo = os.path.join(base, "externo")
    os.makedirs(raiz)
    os.makedirs(externo)
    gravar(externo, "SEGREDO_EXTERNO.txt", MARCADOR_EXTERNO * 200)
    esperado = {}
    modificaveis = []
    esc = 1.0 if real else 0.12
    voc = vocabulario(rng)

    def novo(rel, dados, classe, modif=True, modo=0o644):
        gravar(os.path.join(raiz, "v1"), rel, dados, modo)
        esperado[os.path.join("v1", rel)] = classe
        if modif:
            modificaveis.append(rel)

    def fatia(caminho, ini, n):
        with open(caminho, "rb") as f:
            f.seek(ini)
            return f.read(n)

    if real and silesia:
        d = os.path.join(silesia, "dickens")
        for k in range(4):
            novo("texto/livros/dickens_%d.txt" % (k + 1), fatia(d, k * 2500000, 2500000), "texto")
        w = os.path.join(silesia, "webster")
        for k in range(3):
            novo("texto/dicionario/webster_%d.txt" % (k + 1), fatia(w, k * 3 * MIB, 3 * MIB), "texto")
        novo("texto/xml/docs.xml", ler(os.path.join(silesia, "xml")), None)
        novo("codigo/samba.tar", ler(os.path.join(silesia, "samba")), None)
        novo("binarios/mozilla_parte.tar", fatia(os.path.join(silesia, "mozilla"), 0, 12 * MIB), "binario")
        novo("binarios/ooffice.dll", ler(os.path.join(silesia, "ooffice")), "binario")
        novo("bancos/osdb.dat", ler(os.path.join(silesia, "osdb")), None)
        xr = os.path.join(silesia, "x-ray")
        tam = os.path.getsize(xr)
        for k in range(3):
            novo("cientifico/scan_%d.raw" % (k + 1), fatia(xr, k * (tam // 3), tam // 3), "cientifico_numerico")
    else:
        for k in range(4):
            novo("texto/livros/capitulos_%d.txt" % (k + 1), texto_sintetico(rng, int(2.5 * MIB * esc), voc), "texto")
        for k in range(3):
            novo("texto/dicionario/verbetes_%d.txt" % (k + 1), texto_sintetico(rng, int(3 * MIB * esc), voc), "texto")
        for k in range(3):
            novo("cientifico/scan_%d.raw" % (k + 1), serie_int16(rng, int(1.3 * MIB * esc / 2 * 2)), "cientifico_numerico")
    if real and canterbury:
        novo("texto/world192.txt", ler(os.path.join(canterbury, "world192.txt")), "texto")
        novo("texto/bible.txt", ler(os.path.join(canterbury, "bible.txt")), "texto")
    for k, p in enumerate(py_stdlib(40 if real else 12, 20 * KIB)):
        novo("codigo/mod_%02d.py" % k, ler(p), "codigo")
    for k in range(6 if real else 4):
        novo("csv/vendas_2026-%02d.csv" % (k + 1), csv_sintetico(rng, int(25000 * esc) if real else 9000, k + 1), "csv_tabular")
    for k in range(4):
        novo("logs/app-2026-11-%02d.log" % (k + 1), log_sintetico(rng, int(30000 * esc) if real else 6000, k + 1), "log")
    for k in range(3):
        novo("logs/eventos_%d.ndjson" % (k + 1), ndjson_sintetico(rng, int(25000 * esc) if real else 5000), "json_ndjson")
    for k, p in enumerate(elfs(8 if real else 5)):
        novo("binarios/bin_%02d" % k, ler(p), "binario")
    for k in range(4):
        novo("cientifico/sensor_%d.dat" % (k + 1), serie_float(rng, int(380000 * esc) if real else 40000), "cientifico_numerico")
    caminho_db = os.path.join(base, "_semente.sqlite")
    criar_sqlite(caminho_db, rng, 25000 if real else 6000)
    novo("bancos/clientes.sqlite", ler(caminho_db), "banco_de_dados")
    for k in range(3):
        novo("aleatorio/cofre_%d.bin" % (k + 1), rng.bytes(int(2 * MIB * esc)), "criptografado_aleatorio", modif=False)
    novo("comprimidos/livro.xz", lzma.compress(texto_sintetico(rng, int(2 * MIB * esc), voc)), "container_comprimido", modif=False)
    novo("comprimidos/logs.gz", gzip.compress(log_sintetico(rng, 8000, 9)), "container_comprimido", modif=False)
    zp = os.path.join(base, "_pacote.zip")
    with zipfile.ZipFile(zp, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("a.txt", texto_sintetico(rng, 200 * KIB, voc))
        z.writestr("b.txt", texto_sintetico(rng, 200 * KIB, voc))
    novo("comprimidos/pacote.zip", ler(zp), "container_comprimido", modif=False)
    novo("midia/foto.jpg", b"\xff\xd8\xff\xe0\x00\x10JFIF\x00" + rng.bytes(int(1 * MIB * esc)), "imagem_audio_video", modif=False)
    novo("midia/video.mp4", b"\x00\x00\x00\x20ftypisom\x00\x00\x02\x00isomiso2" + rng.bytes(int(4 * MIB * esc)), "imagem_audio_video", modif=False)
    novo("comprimidos/tabela.parquet", b"PAR1" + rng.bytes(int(1 * MIB * esc)) + b"PAR1", "container_comprimido", modif=False)
    novo("texto/relatório final ação.txt", texto_sintetico(rng, 300 * KIB, voc), "texto")
    novo("dir com espaço/ação #1.txt", texto_sintetico(rng, 120 * KIB, voc), "texto")
    novo("CONFIDENCIAL_cliente_acme/lista.csv", csv_sintetico(rng, 3000, 7), "csv_tabular")
    novo("vazio.txt", b"", "vazio", modif=False)
    inval = os.path.join(os.fsencode(raiz), b"v1", b"inv\xff\xfe_SEGREDO.txt")
    os.makedirs(os.path.dirname(inval), exist_ok=True)
    with open(inval, "wb") as f:
        f.write(texto_sintetico(rng, 90 * KIB, voc))
    esperado[b"v1/inv\xff\xfe_SEGREDO.txt"] = "texto"
    pasta_inv = os.path.join(os.fsencode(raiz), b"v1", b"dir_\xc3\x28_inv")
    os.makedirs(pasta_inv, exist_ok=True)
    with open(os.path.join(pasta_inv, b"arquivo.txt"), "wb") as f:
        f.write(texto_sintetico(rng, 80 * KIB, voc))
    esperado[b"v1/dir_\xc3\x28_inv/arquivo.txt"] = "texto"

    dup_a = ler(os.path.join(raiz, "v1", "aleatorio", "cofre_1.bin"))
    novo("lixo/cofre_1.bin", dup_a, "criptografado_aleatorio", modif=False)
    novo("outro/cofre_1.bin", dup_a, "criptografado_aleatorio", modif=False)
    if real and silesia:
        txt = ler(os.path.join(raiz, "v1", "texto", "livros", "dickens_1.txt"))
        novo("backup/dickens_1_copia.txt", txt, "texto", modif=False)
        novo("backup/dickens_1 (1).txt", txt, "texto", modif=False)
    else:
        txt = ler(os.path.join(raiz, "v1", "texto", "livros", "capitulos_1.txt"))
        novo("backup/capitulos_1_copia.txt", txt, "texto", modif=False)
        novo("backup/capitulos_1 (1).txt", txt, "texto", modif=False)
    tam_igual = int(1 * MIB)
    novo("iguais/mesmo_tamanho_a.bin", rng.bytes(tam_igual), "criptografado_aleatorio", modif=False)
    novo("iguais/mesmo_tamanho_b.bin", rng.bytes(tam_igual), "criptografado_aleatorio", modif=False)
    miolo = bytearray(rng.bytes(tam_igual))
    novo("iguais/quase_igual_a.bin", bytes(miolo), "criptografado_aleatorio", modif=False)
    miolo[tam_igual // 2 + 100000] ^= 0xFF
    novo("iguais/quase_igual_b.bin", bytes(miolo), "criptografado_aleatorio", modif=False)
    novo("trancado/sem_permissao.dat", rng.bytes(64 * KIB), None, modif=False, modo=0o000)
    novo("dir_trancado/dentro.txt", texto_sintetico(rng, 50 * KIB, voc), None, modif=False)
    os.chmod(os.path.join(raiz, "v1", "dir_trancado"), 0o000)
    os.mkfifo(os.path.join(raiz, "v1", "canal.fifo"))
    os.symlink(externo, os.path.join(raiz, "v1", "link_para_fora"))
    os.symlink(os.path.join(externo, "SEGREDO_EXTERNO.txt"), os.path.join(raiz, "v1", "link_arquivo_fora"))
    os.symlink(os.path.join("texto", "relatório final ação.txt"), os.path.join(raiz, "v1", "link_interno"))
    os.symlink("link_laco", os.path.join(raiz, "v1", "link_laco"))

    ignorar_v2 = {"trancado/sem_permissao.dat", "dir_trancado/dentro.txt", "vazio.txt"}
    pares = []
    duplicatas_entre_versoes = []
    removidos = []
    modificados = []
    novos_v2 = []
    for d, ds, fs in os.walk(os.path.join(raiz, "v1")):
        ds.sort()
        for f in sorted(fs):
            p = os.path.join(d, f)
            rel = os.path.relpath(p, os.path.join(raiz, "v1"))
            if os.path.islink(p) or rel in ignorar_v2 or stat.S_ISFIFO(os.lstat(p).st_mode):
                continue
            if rel.startswith("lixo") or rel.startswith("outro") or rel.startswith("backup") or rel.startswith("iguais"):
                continue
            dados = ler(p)
            sorteio = rng.random()
            if rel in modificaveis and sorteio < 0.55:
                if rel.endswith(".sqlite"):
                    destino = os.path.join(raiz, "v2", rel)
                    os.makedirs(os.path.dirname(destino), exist_ok=True)
                    atualizar_sqlite(p, destino, rng, 600)
                else:
                    irmaos = [r for r in modificaveis if r != rel and os.path.splitext(r)[1] == os.path.splitext(rel)[1] and not r.endswith(".sqlite")]
                    fonte = ler(os.path.join(raiz, "v1", irmaos[0])) if irmaos else None
                    gravar(os.path.join(raiz, "v2"), rel, editar(rng, dados, 0.10, fonte))
                pares.append((os.path.join("v1", rel), os.path.join("v2", rel)))
                modificados.append(rel)
            elif sorteio < 0.93:
                gravar(os.path.join(raiz, "v2"), rel, dados)
                duplicatas_entre_versoes.append((os.path.join("v1", rel), os.path.join("v2", rel)))
            else:
                removidos.append(rel)
    for k, p in enumerate(py_stdlib(6 if real else 3, 20 * KIB, 60)):
        gravar(os.path.join(raiz, "v2"), "codigo/novo_%02d.py" % k, ler(p))
        novos_v2.append("codigo/novo_%02d.py" % k)
    gravar(os.path.join(raiz, "v2"), "novos/aleatorio_novo.bin", rng.bytes(int(2 * MIB * esc)))
    gravar(os.path.join(raiz, "v2"), "novos/pacote_novo.xz", lzma.compress(texto_sintetico(rng, int(1 * MIB * esc) + 50000, voc)))
    gravar(os.path.join(raiz, "v2"), "novos/relatorio_novo.txt", texto_sintetico(rng, 400 * KIB, voc))
    for rel in ("novos/aleatorio_novo.bin", "novos/pacote_novo.xz", "novos/relatorio_novo.txt"):
        novos_v2.append(rel)
    esperado[os.path.join("v2", "novos/aleatorio_novo.bin")] = "criptografado_aleatorio"
    esperado[os.path.join("v2", "novos/pacote_novo.xz")] = "container_comprimido"
    esperado[os.path.join("v2", "novos/relatorio_novo.txt")] = "texto"
    for rel, cls in list(esperado.items()):
        if isinstance(rel, str) and rel.startswith("v1" + os.sep) and cls is not None:
            r2 = os.path.join("v2", rel[3:])
            if os.path.exists(os.path.join(raiz, r2)):
                esperado[r2] = cls
    os.unlink(caminho_db)
    os.unlink(zp)
    return {
        "base": base,
        "raiz": raiz,
        "externo": externo,
        "marcador_externo": MARCADOR_EXTERNO,
        "esperado_classe": {k: v for k, v in esperado.items() if v is not None},
        "pares_versoes_modificadas": pares,
        "pares_identicos_entre_versoes": duplicatas_entre_versoes,
        "removidos_em_v2": removidos,
        "novos_em_v2": novos_v2,
        "duplicatas_exatas_dentro_de_v1": [
            ["v1/aleatorio/cofre_1.bin", "v1/lixo/cofre_1.bin", "v1/outro/cofre_1.bin"],
            (["v1/texto/livros/dickens_1.txt", "v1/backup/dickens_1_copia.txt", "v1/backup/dickens_1 (1).txt"] if (real and silesia) else ["v1/texto/livros/capitulos_1.txt", "v1/backup/capitulos_1_copia.txt", "v1/backup/capitulos_1 (1).txt"]),
        ],
        "mesmo_tamanho_diferentes": [("v1/iguais/mesmo_tamanho_a.bin", "v1/iguais/mesmo_tamanho_b.bin"), ("v1/iguais/quase_igual_a.bin", "v1/iguais/quase_igual_b.bin")],
        "ilegivel": "v1/trancado/sem_permissao.dat",
        "dir_ilegivel": "v1/dir_trancado",
        "links": ["v1/link_para_fora", "v1/link_arquivo_fora", "v1/link_interno", "v1/link_laco"],
        "fifo": "v1/canal.fifo",
        "nomes_sensiveis": NOMES_SENSIVEIS,
        "nome_invalido_utf8": [b"v1/inv\xff\xfe_SEGREDO.txt", b"v1/dir_\xc3\x28_inv/arquivo.txt"],
    }


def destruir(base):
    if not os.path.isdir(base):
        return
    os.chmod(base, 0o755)
    for d, ds, fs in os.walk(base):
        for sub in ds:
            caminho = os.path.join(d, sub)
            if not os.path.islink(caminho):
                try:
                    os.chmod(caminho, 0o755)
                except OSError:
                    pass
    shutil.rmtree(base, ignore_errors=True)
