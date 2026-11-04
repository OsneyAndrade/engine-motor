import argparse
import atexit
import bisect
import datetime
import hashlib
import json
import lzma
import math
import os
import platform
import random
import re
import statistics
import struct
import subprocess
import sys
import time
from array import array
from collections import Counter
from compression import zstd

MIB = 1 << 20
IMAGEM = "syntra-openzl:pinned"
COMMIT_FIXADO = "32246b48faee46807f84183dac4db479089f5445"
SEMENTE_PADRAO = 20261104
CPUS_E = "16-31"
MEMORIA = "12g"
TMPFS_SAIDA_MIB = 3072
CAP_REAL = 16 * MIB
BLOCOS_TREINO = 4

SINTETICOS_NOVOS = {
    "sint_vendas.csv": {"classe": "sint_csv_vendas", "alvo_mib": 32},
    "sint_eventos.ndjson": {"classe": "sint_ndjson_eventos", "alvo_mib": 32},
    "sint_telemetria_f32.bin": {"classe": "sint_float32_telemetria", "alvo_mib": 32},
    "sint_timestamps_i64.bin": {"classe": "sint_int64_timestamps", "alvo_mib": 16},
}

CLASSES_ARQUIVO = {
    "dickens": "texto",
    "webster": "texto",
    "reymont": "texto",
    "bible.txt": "texto",
    "world192.txt": "texto",
    "samba": "codigo",
    "mozilla": "binario",
    "ooffice": "binario",
    "sao": "cientifico",
    "nci": "cientifico",
    "x-ray": "cientifico",
    "mr": "cientifico",
    "osdb": "banco_de_dados",
    "xml": "xml",
    "E.coli": "dna",
    "aleatorio.bin": "sint_incompressivel",
    "logs.log": "sint_logs",
    "ja_comprimido.xz": "sint_ja_comprimido",
}

ORIGENS = {
    "silesia": ["dickens", "mozilla", "mr", "nci", "ooffice", "osdb", "reymont", "samba", "sao", "webster", "x-ray", "xml"],
    "canterbury": ["bible.txt", "E.coli", "world192.txt"],
    "sinteticos": ["aleatorio.bin", "logs.log", "ja_comprimido.xz"],
}

ALINHAMENTO = {
    "sao": {"alinhamento": 28, "base": 28},
    "mr": {"alinhamento": 2, "base": 0},
    "x-ray": {"alinhamento": 2, "base": 0},
    "mozilla": {"alinhamento": 8, "base": 0},
    "ooffice": {"alinhamento": 8, "base": 0},
    "osdb": {"alinhamento": 8, "base": 0},
    "E.coli": {"alinhamento": 1, "base": 0},
    "aleatorio.bin": {"alinhamento": 8, "base": 0},
    "ja_comprimido.xz": {"alinhamento": 8, "base": 0},
    "sint_telemetria_f32.bin": {"alinhamento": 4, "base": 0},
    "sint_timestamps_i64.bin": {"alinhamento": 8, "base": 0},
}
POR_LINHA = {
    "dickens", "webster", "reymont", "bible.txt", "world192.txt", "samba", "nci", "xml", "logs.log",
    "sint_vendas.csv", "sint_eventos.ndjson",
}

SDDL_SAO = "/opt/openzl/examples/sddl2/sao_silesia.sddl"

PLANO = {
    "dickens": {"perfis": [], "treinar": ["serial"]},
    "webster": {"perfis": [], "treinar": []},
    "reymont": {"perfis": [], "treinar": []},
    "bible.txt": {"perfis": [], "treinar": []},
    "world192.txt": {"perfis": [], "treinar": []},
    "samba": {"perfis": [], "treinar": ["serial"]},
    "mozilla": {"perfis": [], "treinar": ["serial"]},
    "ooffice": {"perfis": [], "treinar": []},
    "sao": {"perfis": [("sao", None), ("sddl2", None, SDDL_SAO)], "treinar": ["sao", ("sddl2", SDDL_SAO)]},
    "nci": {"perfis": [], "treinar": ["serial"]},
    "x-ray": {"perfis": [("le-u16", None)], "treinar": []},
    "mr": {"perfis": [("le-u16", None)], "treinar": ["le-u16"]},
    "osdb": {"perfis": [], "treinar": ["serial"]},
    "xml": {"perfis": [], "treinar": ["serial"]},
    "E.coli": {"perfis": [("u8", None)], "treinar": []},
    "aleatorio.bin": {"perfis": [], "treinar": []},
    "logs.log": {"perfis": [], "treinar": ["serial"]},
    "ja_comprimido.xz": {"perfis": [], "treinar": []},
    "sint_vendas.csv": {"perfis": [("csv", None)], "treinar": ["csv"]},
    "sint_eventos.ndjson": {"perfis": [], "treinar": ["serial"]},
    "sint_telemetria_f32.bin": {"perfis": [("le-u32", None), ("le-u32", 7)], "treinar": ["le-u32"]},
    "sint_timestamps_i64.bin": {"perfis": [("le-i64", None), ("le-i64", 7)], "treinar": ["le-i64"]},
}

BASELINES = [("zstd-1", "zstd", 1), ("zstd-3", "zstd", 3), ("zstd-19", "zstd", 19), ("xz-9e", "xz", 9)]

REFERENCIAS_PUBLICADAS = [
    {
        "fonte": "doc/mkdocs/doc/index.md do repositorio no commit fixado (tabela SAO, Silesia)",
        "dado": "sao",
        "zstd-3": 1.31,
        "xz-9": 1.64,
        "openzl": 2.06,
        "obs": "REFERENCIA PUBLICADA, nao usada para concluir; arquivo inteiro, perfil sao",
    },
    {
        "fonte": "arXiv:2510.03203, tabela 5 (texto lido localmente)",
        "dado": "enwik7 (10^7 bytes de enwik)",
        "zstd-3": 2.80,
        "zstd-19": 3.58,
        "xz-9": 3.68,
        "openzl": 3.04,
        "obs": "REFERENCIA PUBLICADA, nao usada para concluir; o artigo diz que o compressor treinado usa zstd-6 em texto",
    },
    {
        "fonte": "doc/mkdocs/doc/blog/posts/2026-09-24-compression-transformer.md (v0.3.0)",
        "dado": "868 familias numericas, media geometrica ponderada",
        "zstd-19": 5.752,
        "xz-9": 5.920,
        "openzl_nao_treinado": 6.036,
        "openzl_treinado": 7.846,
        "openzl_transformer_nivel7": 7.760,
        "obs": "REFERENCIA PUBLICADA, nao usada para concluir; dados numericos reais de varias origens, nao os sinteticos daqui",
    },
]

SCRIPT_MEDIR = r"""
cd /saida || exit 90
rm -f o.zl o2.zl o.out o.log od.log o.cbor
zli compress "$@" -o o.zl -f --trace o.cbor --no-stream-preview > o.log 2>&1
rc=$?
if [ "$rc" -ne 0 ]; then
  printf 'rc=%s rcd=-1 zl=0 sha=- det=-\n' "$rc"
  echo '---LOG'
  tail -c 1500 o.log
  exit 0
fi
zli compress "$@" -o o2.zl -f > /dev/null 2>&1
if cmp -s o.zl o2.zl; then det=1; else det=0; fi
zli decompress o.zl -o o.out -f > od.log 2>&1
rcd=$?
sha=$(sha256sum o.out 2>/dev/null | cut -d' ' -f1)
zl=$(stat -c %s o.zl)
shazl=$(sha256sum o.zl | cut -d' ' -f1)
printf 'rc=%s rcd=%s zl=%s sha=%s det=%s shazl=%s\n' "$rc" "$rcd" "$zl" "${sha:--}" "$det" "$shazl"
echo '---LOG'
tail -c 1500 o.log
tail -c 600 od.log
"""

SCRIPT_TREINAR = r"""
cd /saida || exit 90
mkdir -p treinados
saida=$1
shift
rm -f "$saida"
zli train "$@" -o "$saida" -f > treino.log 2>&1
rc=$?
if [ -f "$saida" ]; then tam=$(stat -c %s "$saida"); sha=$(sha256sum "$saida" | cut -d' ' -f1); else tam=0; sha=-; fi
printf 'rc=%s tam=%s sha=%s\n' "$rc" "$tam" "$sha"
echo '---LOG'
tail -c 3000 treino.log
"""


def sha256_bytes(dados):
    return hashlib.sha256(dados).hexdigest()


def sha256_arquivo(caminho):
    h = hashlib.sha256()
    with open(caminho, "rb") as f:
        while True:
            b = f.read(1 << 22)
            if not b:
                break
            h.update(b)
    return h.hexdigest()


def decodificar_cbor(b, i=0):
    ib = b[i]
    mt = ib >> 5
    ai = ib & 31
    i += 1
    if ai < 24:
        val = ai
    elif ai == 24:
        val = b[i]
        i += 1
    elif ai == 25:
        val = struct.unpack(">H", b[i : i + 2])[0]
        i += 2
    elif ai == 26:
        val = struct.unpack(">I", b[i : i + 4])[0]
        i += 4
    elif ai == 27:
        val = struct.unpack(">Q", b[i : i + 8])[0]
        i += 8
    elif ai == 31:
        val = None
    else:
        raise ValueError("cbor ai " + str(ai))
    if mt == 0:
        return val, i
    if mt == 1:
        return -1 - val, i
    if mt == 2:
        return b[i : i + val], i + val
    if mt == 3:
        return b[i : i + val].decode("utf-8", "replace"), i + val
    if mt == 4:
        out = []
        if val is None:
            while b[i] != 0xFF:
                x, i = decodificar_cbor(b, i)
                out.append(x)
            return out, i + 1
        for _ in range(val):
            x, i = decodificar_cbor(b, i)
            out.append(x)
        return out, i
    if mt == 5:
        out = {}
        if val is None:
            while b[i] != 0xFF:
                k, i = decodificar_cbor(b, i)
                v, i = decodificar_cbor(b, i)
                out[k] = v
            return out, i + 1
        for _ in range(val):
            k, i = decodificar_cbor(b, i)
            v, i = decodificar_cbor(b, i)
            out[k] = v
        return out, i
    if mt == 6:
        return decodificar_cbor(b, i)
    if ai == 20:
        return False, i
    if ai == 21:
        return True, i
    if ai == 22:
        return None, i
    return val, i


def resumir_trace(dados):
    try:
        raiz, _ = decodificar_cbor(dados)
    except Exception as e:
        return {"erro": repr(e)}
    codecs = Counter()
    chunks = raiz.get("chunks", [])
    for ch in chunks:
        for c in ch.get("codecs", []):
            nome = re.sub(r"#\d+$", "", c.get("name", "?"))
            codecs[nome] += 1
    return {
        "frame_version": raiz.get("frameVersion"),
        "library_version": raiz.get("libraryVersion"),
        "n_chunks": len(chunks),
        "codecs": dict(sorted(codecs.items())),
    }


ESTRUTURAIS = {
    "zl.delta_int", "zl.field_lz", "zl.split_by_struct", "zl.transpose_split", "zl.tokenize_numeric", "zl.tokenize_struct",
    "zl.private.tokenize_sorted", "zl.range_pack", "zl.sparse_num", "zl.dispatch_string", "zl.separate_string_components",
    "zl.convert_struct_to_num_le", "zl.convert_num_to_struct_le", "zl.zigzag", "zl.float_deconstruct", "zl.private.splitN_struct",
    "zl.merge_sorted", "zl.prefix", "zl.parse_int", "zl.divide_by", "zl.bitsplit_top8", "zl.bitsplit_fp", "zl.bitsplit_bf16",
    "zl.transpose", "zl.tokenize", "zl.quantize_lengths", "zl.quantize_offsets",
}


def grafo_estrutural(codecs):
    return bool(set(codecs) & ESTRUTURAIS)


class Conteiner:
    def __init__(self, rotulo, cpus, fatias, tmpfs_mib=TMPFS_SAIDA_MIB, memoria=MEMORIA):
        self.nome = "syntra-openzl-%s-%d-%d" % (rotulo, os.getpid(), int(time.time()))
        self.cpus = cpus
        self.fatias = fatias
        self.tmpfs_mib = tmpfs_mib
        self.memoria = memoria
        self.ativo = False

    def iniciar(self):
        cmd = [
            "docker", "run", "-d", "--name", self.nome, "--network", "none",
            "--cpuset-cpus=" + self.cpus, "--memory=" + self.memoria, "--memory-swap=" + self.memoria,
            "--pids-limit", "1024", "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
            "--read-only", "--tmpfs", "/tmp:rw,size=256m", "--tmpfs", "/saida:rw,size=%dm" % self.tmpfs_mib,
            "-v", self.fatias + ":/dados:ro", IMAGEM,
        ]
        subprocess.run(cmd, check=True, capture_output=True)
        self.ativo = True
        atexit.register(self.parar)

    def executar(self, args, timeout=None):
        return subprocess.run(["docker", "exec", self.nome] + args, capture_output=True, timeout=timeout)

    def sh(self, script, args, timeout=None):
        return self.executar(["sh", "-c", script, "sh"] + args, timeout=timeout)

    def ler(self, caminho):
        r = self.executar(["cat", caminho])
        return r.stdout if r.returncode == 0 else b""

    def parar(self):
        if self.ativo:
            subprocess.run(["docker", "rm", "-f", self.nome], capture_output=True)
            self.ativo = False


def parsear_saida(saida):
    texto = saida.decode("utf-8", "replace")
    cab, _, log = texto.partition("---LOG")
    campos = dict(p.split("=", 1) for p in cab.split() if "=" in p)
    return campos, log.strip()


def filtrar_log(log):
    linhas = [l for l in log.splitlines() if not re.match(r"^\s*(Compressed|Decompressed)\b", l)]
    return re.sub(r"[0-9.]+ MB/s", "<velocidade descartada>", "\n".join(linhas))[-700:]


def particionar(dados, alinhamento, base, por_linha):
    n = len(dados)
    corte = base + (((n - base) // 2) // alinhamento) * alinhamento
    if por_linha:
        j = dados.find(b"\n", corte)
        if j != -1:
            corte = j + 1
    return corte


def dividir_em_blocos(dados, k, alinhamento, base, por_linha):
    n = len(dados)
    if k <= 1 or n < k * 4096:
        return [dados]
    pos = [0]
    for i in range(1, k):
        c = base + (((n * i // k) - base) // alinhamento) * alinhamento
        c = max(c, pos[-1])
        if por_linha:
            j = dados.find(b"\n", c)
            c = j + 1 if j != -1 else n
        pos.append(c)
    pos.append(n)
    return [dados[pos[i] : pos[i + 1]] for i in range(k) if pos[i + 1] > pos[i]]


NOMES = (
    "Ana Bruno Carla Diego Elisa Fabio Gabriela Hugo Irene Joao Karina Lucas Marina Nelson Olivia Paulo Quiteria Rafael Sofia Tiago "
    "Ursula Vitor Wagner Ximena Yara Zeca Alice Bernardo Camila Daniel Eduarda Felipe Giovana Henrique Isabela Jorge Larissa Marcelo "
    "Natalia Otavio Patricia Renato Sabrina Thiago Valeria Washington Adriana Beatriz Caio Debora Eduardo Fernanda Gustavo Helena "
    "Igor Julia Kleber Leticia Mateus"
).split()
SOBRENOMES = (
    "Silva Santos Oliveira Souza Rodrigues Ferreira Alves Pereira Lima Gomes Costa Ribeiro Martins Carvalho Almeida Lopes Soares "
    "Fernandes Vieira Barbosa Rocha Dias Nascimento Andrade Moreira Nunes Marques Machado Mendes Freitas Cardoso Ramos Goncalves "
    "Santana Teixeira Araujo Pinto Correia Moura Campos Cavalcanti Monteiro Batista Barros Melo Cunha Pires Farias Duarte Reis Miranda "
    "Castro Azevedo Fonseca Medeiros Peixoto Xavier Siqueira Tavares Brandao Lacerda Bezerra Coelho Guimaraes Assis"
).split()


class Gerador:
    def __init__(self, semente):
        self.r = random.Random(semente)
        self.u = self.r.random

    def inteiro(self, n):
        return int(self.u() * n)

    def normal(self):
        a = 1.0 - self.u()
        b = self.u()
        return math.sqrt(-2.0 * math.log(a)) * math.cos(2.0 * math.pi * b)

    def exponencial(self, media):
        return -media * math.log(1.0 - self.u())

    def tabela(self, pesos):
        acum = []
        s = 0.0
        for p in pesos:
            s += p
            acum.append(s)
        return acum, s

    def sorteio(self, acum, total):
        return bisect.bisect_left(acum, self.u() * total)


def zipf(n, s):
    return [1.0 / ((i + 1) ** s) for i in range(n)]


def gerar_csv_vendas(semente, alvo):
    g = Gerador(semente)
    clientes = ["%s %s" % (NOMES[i % len(NOMES)], SOBRENOMES[(i * 7 + i // len(NOMES)) % len(SOBRENOMES)]) for i in range(4000)]
    produtos = ["PRD-%05d" % (10000 + i * 7) for i in range(1500)]
    base_preco = [round(5.0 + 995.0 * (g.u() ** 3), 2) for _ in produtos]
    lojas = ["LOJA-%02d" % i for i in range(40)]
    ac_cli, t_cli = g.tabela(zipf(len(clientes), 1.0))
    ac_prod, t_prod = g.tabela(zipf(len(produtos), 0.9))
    ac_loja, t_loja = g.tabela(zipf(len(lojas), 0.7))
    cabecalho = "data,id,cliente,produto,quantidade,preco,loja\n"
    saida = bytearray(cabecalho.encode("ascii"))
    dia = datetime.date(2025, 1, 1)
    id_atual = 1000000
    restantes = 0
    texto_dia = dia.isoformat()
    descontos = (0.0, 0.0, 0.0, 0.0, 0.05, 0.10, 0.15)
    while True:
        if restantes == 0:
            dia += datetime.timedelta(days=1)
            texto_dia = dia.isoformat()
            restantes = 800 + g.inteiro(400)
        restantes -= 1
        id_atual += 1
        cli = clientes[g.sorteio(ac_cli, t_cli)]
        k = g.sorteio(ac_prod, t_prod)
        qtd = 1
        while qtd < 12 and g.u() < 0.45:
            qtd += 1
        preco = round(base_preco[k] * (1.0 - descontos[g.inteiro(len(descontos))]), 2)
        loja = lojas[g.sorteio(ac_loja, t_loja)]
        linha = ("%s,%d,%s,%s,%d,%.2f,%s\n" % (texto_dia, id_atual, cli, produtos[k], qtd, preco, loja)).encode("ascii")
        if len(saida) + len(linha) > alvo:
            break
        saida += linha
    return bytes(saida)


def gerar_ndjson_eventos(semente, alvo):
    g = Gerador(semente)
    niveis = ["INFO"] * 14 + ["DEBUG"] * 4 + ["WARN"] * 2 + ["ERROR"]
    servicos = ["api-gateway", "auth", "storage-node", "ingest", "scheduler", "billing", "indexer", "cache"]
    eventos = ["login", "logout", "object.put", "object.get", "object.delete", "invoice.created", "invoice.paid", "job.started",
               "job.finished", "cache.miss", "cache.hit", "token.refresh", "index.update", "replica.sync", "quota.check"]
    estados = [200] * 70 + [201] * 8 + [204] * 6 + [304] * 4 + [400] * 3 + [404] * 4 + [500] * 2 + [503]
    hosts = ["node-%02d" % i for i in range(24)]
    ac_u, t_u = g.tabela(zipf(50000, 0.8))
    instante = 1793750400000
    saida = bytearray()
    while True:
        instante += int(g.exponencial(25.0)) + 1
        seg, ms = divmod(instante, 1000)
        t = time.gmtime(seg)
        linha = (
            '{"ts":"%04d-%02d-%02dT%02d:%02d:%02d.%03dZ","level":"%s","service":"%s","host":"%s","event":"%s",'
            '"user_id":%d,"trace_id":"%016x","latency_ms":%d,"status":%d,"bytes":%d}\n'
            % (
                t.tm_year, t.tm_mon, t.tm_mday, t.tm_hour, t.tm_min, t.tm_sec, ms,
                niveis[g.inteiro(len(niveis))], servicos[g.inteiro(len(servicos))], hosts[g.inteiro(len(hosts))],
                eventos[g.inteiro(len(eventos))], 100000 + g.sorteio(ac_u, t_u), (int(g.u() * (1 << 32)) << 32) | int(g.u() * (1 << 32)),
                int(math.exp(2.5 + 1.0 * g.normal())), estados[g.inteiro(len(estados))], int(math.exp(7.0 + 1.6 * g.normal())),
            )
        ).encode("ascii")
        if len(saida) + len(linha) > alvo:
            break
        saida += linha
    return bytes(saida)


def gerar_telemetria_f32(semente, alvo):
    g = Gerador(semente)
    sensores = 16
    amostras = alvo // 4 // sensores
    valores = array("f")
    resolucoes = (0.01, 0.01, 0.1, 0.5)
    for s in range(sensores):
        base = -50.0 + 200.0 * g.u()
        inclinacao = (g.u() - 0.5) * 2e-4
        amplitude = 0.5 + 19.5 * g.u()
        periodo = 5000.0 + 55000.0 * g.u()
        fase = 2.0 * math.pi * g.u()
        sigma = 0.01 + 0.49 * g.u()
        res = resolucoes[s % len(resolucoes)]
        ruido = 0.0
        phi = 0.95
        inov = sigma * math.sqrt(1.0 - phi * phi)
        for t in range(amostras):
            ruido = phi * ruido + inov * g.normal()
            v = base + inclinacao * t + amplitude * math.sin(2.0 * math.pi * t / periodo + fase) + ruido
            valores.append(round(v / res) * res)
    if sys.byteorder != "little":
        valores.byteswap()
    return valores.tobytes()


def gerar_timestamps_i64(semente, alvo):
    g = Gerador(semente)
    n = alvo // 8
    valores = array("q")
    t = 1793750400000
    for _ in range(n):
        u = g.u()
        if u < 0.01:
            t += int(g.inteiro(2))
        elif u < 0.0101:
            t += 1000 + g.inteiro(5000)
        else:
            t += 1 + int(g.exponencial(40.0))
        valores.append(t)
    if sys.byteorder != "little":
        valores.byteswap()
    return valores.tobytes()


GERADORES = {
    "sint_vendas.csv": (gerar_csv_vendas, 1),
    "sint_eventos.ndjson": (gerar_ndjson_eventos, 2),
    "sint_telemetria_f32.bin": (gerar_telemetria_f32, 3),
    "sint_timestamps_i64.bin": (gerar_timestamps_i64, 4),
}

DESCRICAO_SINTETICOS = {
    "sint_vendas.csv": "CSV de vendas, 7 colunas: data crescente (800 a 1199 linhas por dia a partir de 2025-01-01), id sequencial (inicia em 1000001), cliente categorico (4000 nomes, zipf s=1,0), produto categorico (1500 codigos PRD-nnnnn, zipf s=0,9), quantidade inteira pequena (1 a 12, geometrica p=0,45), preco decimal com 2 casas (preco base por produto, desconto 0/5/10/15%), loja categorica (40, zipf s=0,7)",
    "sint_eventos.ndjson": "NDJSON de eventos de log com 10 campos uniformes na mesma ordem (ts ISO em ms crescente, level, service, host, event, user_id zipf, trace_id aleatorio de 64 bits, latency_ms lognormal, status, bytes lognormal)",
    "sint_telemetria_f32.bin": "float32 little-endian: 16 series de sensores contiguas (sensor-major), 524288 amostras cada, base + tendencia linear + senoide + ruido AR(1) phi=0,95, quantizadas na resolucao do sensor (0,01; 0,01; 0,1; 0,5 em rodizio)",
    "sint_timestamps_i64.bin": "int64 little-endian de timestamps em ms crescentes a partir de 1793750400000: 98,99% com passo 1 + exponencial(media 40), 1% com passo 0 ou 1, 0,01% com salto de 1 a 6 s",
}


def carregar_medicoes(caminho):
    with open(caminho) as f:
        return json.load(f)


def caminho_corpus(dados, nome):
    for pasta, nomes in ORIGENS.items():
        if nome in nomes:
            return os.path.join(dados, pasta, nome)
    return os.path.join(dados, "sinteticos_openzl", nome)


def conferir_dados(dados, medicoes):
    saida = []
    ok = True
    for a in medicoes["arquivos"]:
        p = caminho_corpus(dados, a["nome"])
        if not os.path.exists(p):
            saida.append({"nome": a["nome"], "presente": False})
            ok = False
            continue
        h = sha256_arquivo(p)
        igual = h == a["sha256_arquivo"] and os.path.getsize(p) == a["tamanho_bytes"]
        ok = ok and igual
        saida.append({"nome": a["nome"], "presente": True, "sha256": h, "confere_com_medicoes_json": igual})
    return ok, saida


def cmd_preparar(a):
    medicoes = carregar_medicoes(a.medicoes)
    ok, conf = conferir_dados(a.dados, medicoes)
    if not ok:
        print("dados nao conferem com medicoes.json:", [c for c in conf if not c.get("confere_com_medicoes_json")], file=sys.stderr)
        sys.exit(2)
    pasta_sint = os.path.join(a.dados, "sinteticos_openzl")
    os.makedirs(pasta_sint, exist_ok=True)
    info_sint = {}
    for nome, (fn, desloc) in GERADORES.items():
        alvo = SINTETICOS_NOVOS[nome]["alvo_mib"] * MIB
        caminho = os.path.join(pasta_sint, nome)
        t0 = time.time()
        dados = fn(a.semente * 10 + desloc, alvo)
        with open(caminho, "wb") as f:
            f.write(dados)
        info_sint[nome] = {
            "classe": SINTETICOS_NOVOS[nome]["classe"],
            "sintetico": True,
            "semente": a.semente * 10 + desloc,
            "descricao": DESCRICAO_SINTETICOS[nome],
            "tamanho_bytes": len(dados),
            "sha256": sha256_bytes(dados),
            "segundos_geracao": round(time.time() - t0, 1),
        }
        print("gerado", nome, len(dados), info_sint[nome]["sha256"], file=sys.stderr)
    if os.path.exists(a.fatias):
        if os.listdir(a.fatias):
            sys.exit("diretorio de fatias precisa estar vazio: " + a.fatias)
    os.makedirs(a.fatias, exist_ok=True)
    itens = []
    nomes = list(CLASSES_ARQUIVO) + list(SINTETICOS_NOVOS)
    for nome in nomes:
        caminho = caminho_corpus(a.dados, nome)
        with open(caminho, "rb") as f:
            inteiro = f.read()
        sintetico_novo = nome in SINTETICOS_NOVOS
        cap = len(inteiro) if sintetico_novo else CAP_REAL
        fatia = inteiro[: min(len(inteiro), cap)]
        alin = ALINHAMENTO.get(nome, {"alinhamento": 1, "base": 0})
        por_linha = nome in POR_LINHA
        corte = particionar(fatia, alin["alinhamento"], alin["base"], por_linha)
        treino = fatia[:corte]
        teste = fatia[corte:]
        cabecalho = b""
        if nome == "sint_vendas.csv":
            cabecalho = fatia[: fatia.index(b"\n") + 1]
            teste = cabecalho + teste
        amostras = dividir_em_blocos(treino, BLOCOS_TREINO, alin["alinhamento"], alin["base"], por_linha)
        if cabecalho:
            amostras = [x if x.startswith(cabecalho) else cabecalho + x for x in amostras]
        pasta = os.path.join(a.fatias, nome)
        os.makedirs(os.path.join(pasta, "treino"))
        with open(os.path.join(pasta, "teste.bin"), "wb") as f:
            f.write(teste)
        for i, x in enumerate(amostras):
            with open(os.path.join(pasta, "treino", "amostra_%02d.bin" % i), "wb") as f:
                f.write(x)
        classe = SINTETICOS_NOVOS[nome]["classe"] if sintetico_novo else CLASSES_ARQUIVO[nome]
        sintetico = sintetico_novo or classe.startswith("sint_")
        itens.append(
            {
                "id": nome,
                "classe": classe,
                "sintetico": sintetico,
                "arquivo_inteiro_bytes": len(inteiro),
                "fatia_total_bytes": len(fatia),
                "divisao": "treino = fatia[0:%d], teste = fatia[%d:%d]%s" % (
                    corte, corte, len(fatia), " com cabecalho CSV repetido no teste e em cada amostra" if cabecalho else ""),
                "treino_bytes": sum(len(x) for x in amostras),
                "n_amostras_treino": len(amostras),
                "teste_bytes": len(teste),
                "sha256_teste": sha256_bytes(teste),
                "sha256_treino": [sha256_bytes(x) for x in amostras],
                "alinhamento": alin,
                "cortes_por_linha": por_linha,
                "sem_sobreposicao": True,
            }
        )
        print("fatias", nome, "treino", sum(len(x) for x in amostras), "teste", len(teste), file=sys.stderr)
    manifesto = {
        "data": time.strftime("%Y-%m-%d %H:%M:%S"),
        "semente_base": a.semente,
        "conferencia_dados": conf,
        "sinteticos_novos": info_sint,
        "itens": itens,
    }
    with open(os.path.join(a.fatias, "manifesto_fatias.json"), "w") as f:
        json.dump(manifesto, f, indent=1)
    print("ok", len(itens), "itens")


def baseline(rotulo, tipo, nivel, dados):
    if tipo == "zstd":
        comp = zstd.compress(dados, level=nivel)
        rec = zstd.decompress(comp)
    else:
        preset = nivel | lzma.PRESET_EXTREME
        comp = lzma.compress(dados, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64, preset=preset)
        rec = lzma.decompress(comp)
    exato = sha256_bytes(rec) == sha256_bytes(dados)
    return {"bytes_comp": len(comp), "razao": len(dados) / len(comp), "bit_exato": exato}


def normalizar_perfil(e):
    e = tuple(e) + (None,) * (3 - len(e))
    return e[0], e[1], e[2]


def normalizar_treino(e):
    if isinstance(e, str):
        return e, None
    return e[0], e[1]


def rotulo_perfil(perfil, nivel, arg):
    r = perfil
    if arg:
        r += "(" + os.path.basename(arg) + ")"
    if nivel is not None:
        r += "@L%d" % nivel
    return r


def medir_openzl(cont, caminho, bytes_orig, sha_orig, perfil=None, nivel=None, compressor=None, estrito=True, perfil_arg=None):
    args = []
    if perfil:
        args += ["--profile", perfil]
    if perfil_arg:
        args += ["--profile-arg", perfil_arg]
    if compressor:
        args += ["--compressor", compressor]
    if nivel is not None:
        args += ["--level", str(nivel)]
    if estrito:
        args += ["--strict"]
    args += [caminho]
    r = cont.sh(SCRIPT_MEDIR, args, timeout=1800)
    campos, log = parsear_saida(r.stdout)
    rc = int(campos.get("rc", "-1"))
    res = {"argumentos": args, "estrito": estrito}
    if rc != 0:
        res.update({"falhou": True, "rc": rc, "log": filtrar_log(log)})
        return res
    trace = resumir_trace(cont.ler("/saida/o.cbor"))
    zl = int(campos["zl"])
    res.update(
        {
            "falhou": False,
            "bytes_comp": zl,
            "razao": bytes_orig / zl,
            "bit_exato": campos.get("rcd") == "0" and campos.get("sha") == sha_orig,
            "determinismo_duas_compressoes_identicas": campos.get("det") == "1",
            "sha256_quadro": campos.get("shazl"),
            "trace": trace,
            "grafo_estrutural": grafo_estrutural(trace.get("codecs", {})),
            "log": filtrar_log(log),
        }
    )
    return res


def medir_com_fallback(cont, caminho, bytes_orig, sha_orig, perfil, nivel, compressor=None, perfil_arg=None):
    r = medir_openzl(cont, caminho, bytes_orig, sha_orig, perfil, nivel, compressor, estrito=True, perfil_arg=perfil_arg)
    if r.get("falhou"):
        r2 = medir_openzl(cont, caminho, bytes_orig, sha_orig, perfil, nivel, compressor, estrito=False, perfil_arg=perfil_arg)
        r2["modo_estrito_falhou"] = True
        r2["erro_estrito"] = r.get("log")
        return r2
    r["modo_estrito_falhou"] = False
    return r


def extrair_ganhos_treino(log):
    pontos = re.findall(r"Training improved compression ratio by (-?[0-9.]+)%", log)
    antes_depois = re.findall(r"(\d+) files: (\d+) -> (\d+) \(([0-9.]+)\)", log)
    return {
        "ganho_in_sample_pct": float(pontos[-1]) if pontos else None,
        "benchmarks_in_sample": [
            {"arquivos": int(x[0]), "bytes_orig": int(x[1]), "bytes_comp": int(x[2]), "razao": float(x[3])} for x in antes_depois
        ],
    }


def treinar(cont, id_item, perfil, nivel, threads, max_s, perfil_arg=None):
    destino = "/saida/treinados/%s__%s.zlc" % (re.sub(r"[^A-Za-z0-9_.-]", "_", id_item), re.sub(r"[^A-Za-z0-9_.-]", "_", rotulo_perfil(perfil, None, perfil_arg)))
    args = [destino, "--profile", perfil]
    if perfil_arg:
        args += ["--profile-arg", perfil_arg]
    args += ["--threads", str(threads), "--max-time-secs", str(max_s), "/dados/%s/treino" % id_item]
    t0 = time.monotonic()
    r = cont.sh(SCRIPT_TREINAR, args, timeout=max_s * 4 + 600)
    dur = time.monotonic() - t0
    campos, log = parsear_saida(r.stdout)
    res = {
        "perfil": perfil,
        "perfil_arg": perfil_arg,
        "rc": int(campos.get("rc", "-1")),
        "tamanho_compressor_bytes": int(campos.get("tam", "0")),
        "sha256_compressor": campos.get("sha"),
        "tempo_relogio_s": round(dur, 1),
        "max_time_secs": max_s,
        "atingiu_limite_de_tempo": dur >= max_s,
        "threads": threads,
        "destino": destino,
        "log": filtrar_log(log),
    }
    res.update(extrair_ganhos_treino(log))
    return res


def versoes_ambiente(cont):
    def cap(args):
        r = cont.executar(args)
        return (r.stdout + r.stderr).decode("utf-8", "replace").strip().splitlines()
    zv = cap(["zli", "--version"])
    return {
        "zli": zv[0] if zv else "",
        "imagem": IMAGEM,
        "commit_openzl": COMMIT_FIXADO,
        "debian": cap(["sh", "-c", "cat /etc/debian_version"]),
        "zstd_python": zstd.zstd_version,
        "python": platform.python_version(),
        "xz_cli": subprocess.run(["xz", "--version"], capture_output=True, text=True).stdout.splitlines()[0],
    }


def carregar_manifesto(fatias):
    with open(os.path.join(fatias, "manifesto_fatias.json")) as f:
        return json.load(f)


def gravar_json(caminho, obj):
    tmp = caminho + ".tmp"
    with open(tmp, "w") as f:
        json.dump(obj, f, indent=1)
    os.replace(tmp, caminho)


def conferir_zstd3(dados, medicoes, resultado):
    por_nome = {}
    for x in medicoes["principal"]:
        if x["modo"] == "inteiro" and x["config"] == "zstd-3":
            por_nome[x["arquivo"]] = x
    saida = []
    for a in medicoes["arquivos"]:
        if a["nome"] not in por_nome or a["nome"] not in CLASSES_ARQUIVO:
            continue
        p = caminho_corpus(dados, a["nome"])
        with open(p, "rb") as f:
            fatia = f.read(a["tamanho_fatia_bytes"])
        comp = zstd.compress(fatia, level=3)
        ref = por_nome[a["nome"]]["bytes_comp"]
        saida.append(
            {
                "arquivo": a["nome"],
                "sha256_fatia_confere": sha256_bytes(fatia) == a["sha256_fatia"],
                "zstd3_bytes_aqui": len(comp),
                "zstd3_bytes_medicoes_json": ref,
                "identico": len(comp) == ref,
            }
        )
    resultado["conferencia_zstd3_vs_medicoes"] = saida


def cmd_razao(a):
    try:
        os.sched_setaffinity(0, set(range(16, 32)))
    except Exception:
        pass
    manifesto = carregar_manifesto(a.fatias)
    medicoes = carregar_medicoes(a.medicoes)
    retomado = {}
    if a.retomar and os.path.exists(a.saida):
        with open(a.saida) as f:
            retomado = json.load(f)
    resultado = retomado or {
        "data": time.strftime("%Y-%m-%d %H:%M:%S"),
        "protocolo": {
            "cpus_conteiner": CPUS_E,
            "memoria": MEMORIA,
            "rede": "none",
            "dados": "somente leitura",
            "saida": "tmpfs",
            "tempo_medido": False,
            "nivel_openzl_padrao": 6,
            "treino": "primeira metade da fatia; teste: segunda metade; sem sobreposicao",
            "treino_max_time_secs": a.treino_max_s,
            "treino_threads": a.threads,
            "baselines": "zstd 1/3/19 via compression.zstd (sem checksum), xz -9e via lzma (CRC64), mesma fatia de teste",
            "openzl_checksums": "conteudo e comprimido, ligados por padrao na biblioteca",
        },
        "itens": [],
        "falhas": [],
    }
    resultado["manifesto_fatias_resumo"] = {"n_itens": len(manifesto["itens"]), "sinteticos_novos": manifesto["sinteticos_novos"]}
    resultado["conferencia_dados"] = manifesto["conferencia_dados"]
    conferir_zstd3(a.dados, medicoes, resultado)
    existentes = {i["id"]: i for i in resultado["itens"]}
    cont = Conteiner("razao", CPUS_E, os.path.abspath(a.fatias))
    cont.iniciar()
    resultado["ambiente"] = versoes_ambiente(cont)
    violacoes = resultado.setdefault("violacoes_bit_exatidao", [])
    try:
        for it in manifesto["itens"]:
            if a.somente and it["id"] not in a.somente:
                continue
            if it["id"] in existentes and not a.somente:
                continue
            caminho_teste = "/dados/%s/teste.bin" % it["id"]
            local = os.path.join(a.fatias, it["id"], "teste.bin")
            with open(local, "rb") as f:
                dados = f.read()
            sha = sha256_bytes(dados)
            if sha != it["sha256_teste"]:
                resultado["falhas"].append({"id": it["id"], "erro": "sha256 do teste diverge do manifesto"})
                continue
            novo = it["id"] not in existentes
            item = existentes.get(it["id"]) or {k: it[k] for k in ("id", "classe", "sintetico", "teste_bytes", "treino_bytes", "n_amostras_treino", "divisao", "sha256_teste")}
            item.setdefault("baselines", {})
            item.setdefault("openzl", {})
            item.setdefault("treinos", {})
            for rot, tipo, nivel in BASELINES:
                if rot in item["baselines"]:
                    continue
                item["baselines"][rot] = baseline(rot, tipo, nivel, dados)
                if not item["baselines"][rot]["bit_exato"]:
                    violacoes.append({"id": it["id"], "config": rot})
            plano = PLANO[it["id"]]
            configs = [("generico:serial", "serial", None, None), ("generico:serial@L9", "serial", 9, None)]
            for e in plano["perfis"]:
                perfil, nivel, arg = normalizar_perfil(e)
                configs.append(("perfil:" + rotulo_perfil(perfil, nivel, arg), perfil, nivel, arg))
            for rot, perfil, nivel, arg in configs:
                if rot in item["openzl"]:
                    continue
                r = medir_com_fallback(cont, caminho_teste, len(dados), sha, perfil, nivel, perfil_arg=arg)
                item["openzl"][rot] = r
                if not r.get("falhou") and not r.get("bit_exato"):
                    violacoes.append({"id": it["id"], "config": rot})
                print(it["id"], rot, r.get("bytes_comp"), "%.4f" % r.get("razao", 0), file=sys.stderr, flush=True)
            if not a.sem_treino:
                for e in plano["treinar"]:
                    perfil, arg = normalizar_treino(e)
                    rot_t = "treinado:" + rotulo_perfil(perfil, None, arg)
                    if rot_t in item["treinos"]:
                        continue
                    tr = treinar(cont, it["id"], perfil, None, a.threads, a.treino_max_s, perfil_arg=arg)
                    item["treinos"][rot_t] = tr
                    print(it["id"], "treino", rot_t, tr["rc"], tr["tempo_relogio_s"], tr.get("ganho_in_sample_pct"), file=sys.stderr, flush=True)
                    if tr["rc"] != 0 or tr["tamanho_compressor_bytes"] == 0:
                        resultado["falhas"].append({"id": it["id"], "etapa": "treino", "perfil": perfil, "log": tr["log"]})
                        continue
                    r = medir_com_fallback(cont, caminho_teste, len(dados), sha, None, None, compressor=tr["destino"])
                    r["perfil_treinado"] = rotulo_perfil(perfil, None, arg)
                    item["openzl"][rot_t] = r
                    if not r.get("falhou") and not r.get("bit_exato"):
                        violacoes.append({"id": it["id"], "config": rot_t})
                    print(it["id"], rot_t, r.get("bytes_comp"), "%.4f" % r.get("razao", 0), file=sys.stderr, flush=True)
            if novo:
                resultado["itens"].append(item)
                existentes[it["id"]] = item
            gravar_json(a.saida, resultado)
    finally:
        cont.parar()
    gravar_json(a.saida, resultado)
    print("violacoes de bit-exatidao:", len(violacoes), "falhas:", len(resultado["falhas"]))


def cmd_determinismo_treino(a):
    try:
        os.sched_setaffinity(0, set(range(16, 32)))
    except Exception:
        pass
    manifesto = carregar_manifesto(a.fatias)
    it = next(i for i in manifesto["itens"] if i["id"] == a.id)
    dados = open(os.path.join(a.fatias, a.id, "teste.bin"), "rb").read()
    sha = sha256_bytes(dados)
    cont = Conteiner("detrain", CPUS_E, os.path.abspath(a.fatias))
    cont.iniciar()
    saida = {"id": a.id, "perfil": a.perfil, "rodadas": []}
    try:
        for k in range(a.rodadas):
            tr = treinar(cont, a.id, a.perfil, None, a.threads, a.treino_max_s)
            r = medir_com_fallback(cont, "/dados/%s/teste.bin" % a.id, len(dados), sha, None, None, compressor=tr["destino"])
            saida["rodadas"].append(
                {
                    "sha256_compressor": tr["sha256_compressor"],
                    "tamanho_compressor_bytes": tr["tamanho_compressor_bytes"],
                    "tempo_relogio_s": tr["tempo_relogio_s"],
                    "bytes_comp_teste": r.get("bytes_comp"),
                    "razao_teste": r.get("razao"),
                    "bit_exato": r.get("bit_exato"),
                }
            )
    finally:
        cont.parar()
    gravar_json(a.saida, saida)
    print(json.dumps(saida, indent=1))


def mediana_tres(fn, repeticoes):
    valores = [fn() for _ in range(repeticoes)]
    return {"valores": valores, "mediana": statistics.median(valores)}


def cmd_tempo(a):
    manifesto = carregar_manifesto(a.fatias)
    itens = [i for i in manifesto["itens"] if not a.somente or i["id"] in a.somente]
    plano_exec = []
    for it in itens:
        plano_exec.append({"id": it["id"], "teste_bytes": it["teste_bytes"], "configs": ["generico:serial"] + [
            "perfil:" + rotulo_perfil(*normalizar_perfil(e)) for e in PLANO[it["id"]]["perfis"]]})
    if a.dry_run:
        print(json.dumps({"cpu_p_core": a.cpu, "repeticoes": a.repeticoes, "plano": plano_exec}, indent=1))
        return
    if not a.confirmo_janela_exclusiva:
        sys.exit("recusado: a medicao de tempo exige --confirmo-janela-exclusiva (maquina sem outras cargas)")
    with open("/proc/loadavg") as f:
        carga = float(f.read().split()[0])
    if carga > a.carga_maxima:
        sys.exit("recusado: carga da maquina %.2f acima de %.2f" % (carga, a.carga_maxima))
    os.sched_setaffinity(0, {int(a.cpu)})
    resultado = {"data": time.strftime("%Y-%m-%d %H:%M:%S"), "cpu": a.cpu, "repeticoes": a.repeticoes, "carga_inicial": carga, "itens": []}
    cont = Conteiner("tempo", str(a.cpu), os.path.abspath(a.fatias), memoria=MEMORIA)
    cont.iniciar()
    padrao_c = re.compile(r"Compressed \d+ -> \d+ \([0-9.]+x\) in ([0-9.]+) ms")
    padrao_d = re.compile(r"in ([0-9.]+) ms")
    try:
        for it in itens:
            local = os.path.join(a.fatias, it["id"], "teste.bin")
            with open(local, "rb") as f:
                dados = f.read()
            item = {"id": it["id"], "teste_bytes": len(dados), "baselines": {}, "openzl": {}}
            for rot, tipo, nivel in BASELINES:
                tc = []
                td = []
                for _ in range(a.repeticoes):
                    t0 = time.perf_counter_ns()
                    comp = zstd.compress(dados, level=nivel) if tipo == "zstd" else lzma.compress(
                        dados, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64, preset=nivel | lzma.PRESET_EXTREME)
                    t1 = time.perf_counter_ns()
                    rec = zstd.decompress(comp) if tipo == "zstd" else lzma.decompress(comp)
                    t2 = time.perf_counter_ns()
                    tc.append((t1 - t0) / 1e9)
                    td.append((t2 - t1) / 1e9)
                    if sha256_bytes(rec) != sha256_bytes(dados):
                        sys.exit("bit-exatidao violada em " + rot)
                item["baselines"][rot] = {
                    "comp_MBps_mediana": len(dados) / 1e6 / statistics.median(tc),
                    "decomp_MBps_mediana": len(dados) / 1e6 / statistics.median(td),
                }
            configs = [("generico:serial", "serial", None, None)] + [
                ("perfil:" + rotulo_perfil(*normalizar_perfil(e)), normalizar_perfil(e)[0], normalizar_perfil(e)[1], normalizar_perfil(e)[2])
                for e in PLANO[it["id"]]["perfis"]]
            for rot, perfil, nivel, arg in configs:
                cargs = (
                    ["--profile", perfil]
                    + (["--profile-arg", arg] if arg else [])
                    + (["--level", str(nivel)] if nivel is not None else [])
                    + ["/dados/%s/teste.bin" % it["id"]]
                )
                script = (
                    "cd /saida || exit 90\n"
                    "zli compress \"$@\" -o t.zl -f > tc.log 2>&1\n"
                    "zli decompress t.zl -o t.out -f > td.log 2>&1\n"
                    "cat tc.log td.log\n"
                    "sha256sum t.out | cut -d' ' -f1\n"
                )
                cs = []
                ds = []
                for _ in range(a.repeticoes):
                    r = cont.executar(["taskset", "-c", str(a.cpu), "sh", "-c", script, "sh"] + cargs, timeout=1800)
                    linhas = r.stdout.decode("utf-8", "replace").splitlines()
                    mc = next((padrao_c.search(l) for l in linhas if padrao_c.search(l)), None)
                    md = next((padrao_d.search(l) for l in linhas if l.startswith("Decompressed") and padrao_d.search(l)), None)
                    sha_ok = bool(linhas) and linhas[-1].strip() == sha256_bytes(dados)
                    if not (mc and md and sha_ok):
                        sys.exit("falha ao medir %s %s" % (it["id"], rot))
                    cs.append(len(dados) / 1e6 / (float(mc.group(1)) / 1000.0))
                    ds.append(len(dados) / 1e6 / (float(md.group(1)) / 1000.0))
                item["openzl"][rot] = {
                    "comp_MBps_mediana": statistics.median(cs),
                    "decomp_MBps_mediana": statistics.median(ds),
                    "comp_MBps_todos": cs,
                    "decomp_MBps_todos": ds,
                }
            resultado["itens"].append(item)
            gravar_json(a.saida, resultado)
    finally:
        cont.parar()
    gravar_json(a.saida, resultado)


def melhor(configs):
    melhor_r = None
    melhor_k = None
    for k, v in configs.items():
        if v.get("falhou") or v.get("razao") is None:
            continue
        if melhor_r is None or v["razao"] > melhor_r:
            melhor_r = v["razao"]
            melhor_k = k
    return melhor_k, melhor_r


def ganho(a, b):
    if a is None or b is None or b == 0:
        return None
    return (a / b - 1.0) * 100.0


def fmt(x, casas=3):
    if x is None:
        return "-"
    return ("%." + str(casas) + "f") % x


def fmt_pct(x):
    if x is None:
        return "-"
    return "%+.1f%%" % x


def agrupar_modo(item):
    grupos = {"generico": {}, "perfil": {}, "treinado": {}}
    for k, v in item["openzl"].items():
        grupos[k.split(":")[0]][k] = v
    return grupos


def limpar_velocidades(obj):
    if isinstance(obj, dict):
        return {k: limpar_velocidades(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [limpar_velocidades(v) for v in obj]
    if isinstance(obj, str):
        return re.sub(r"[0-9.]+ MB/s", "<velocidade descartada>", obj)
    return obj


def construir_relatorio(razao, tempo):
    linhas_item = []
    por_classe = {}
    for it in razao["itens"]:
        bl = it["baselines"]
        g = agrupar_modo(it)
        mg, rg = melhor(g["generico"])
        mp, rp = melhor(g["perfil"])
        mt, rt = melhor(g["treinado"])
        mb, rb = melhor(it["openzl"])
        linha = {
            "id": it["id"],
            "classe": it["classe"],
            "sintetico": it["sintetico"],
            "teste_bytes": it["teste_bytes"],
            "razao": {
                "zstd-1": bl["zstd-1"]["razao"],
                "zstd-3": bl["zstd-3"]["razao"],
                "zstd-19": bl["zstd-19"]["razao"],
                "xz-9e": bl["xz-9e"]["razao"],
                "openzl_generico_L6": it["openzl"]["generico:serial"]["razao"],
                "openzl_generico": rg,
                "openzl_perfil": rp,
                "openzl_treinado": rt,
                "openzl_melhor": rb,
            },
            "melhor_configuracao": mb,
            "ganho_pct": {
                "sobre_zstd-3": ganho(rb, bl["zstd-3"]["razao"]),
                "sobre_zstd-19": ganho(rb, bl["zstd-19"]["razao"]),
                "sobre_xz-9e": ganho(rb, bl["xz-9e"]["razao"]),
            },
            "ganho_generico_pct": {
                "sobre_zstd-3": ganho(rg, bl["zstd-3"]["razao"]),
                "sobre_zstd-19": ganho(rg, bl["zstd-19"]["razao"]),
                "sobre_xz-9e": ganho(rg, bl["xz-9e"]["razao"]),
            },
            "estrito_falhou": [k for k, v in it["openzl"].items() if v.get("modo_estrito_falhou")],
            "sem_codec_estrutural": [k for k, v in it["openzl"].items() if not k.startswith("generico") and not v.get("falhou") and not v.get("grafo_estrutural")],
            "bytes_comp": {
                "zstd-3": bl["zstd-3"]["bytes_comp"],
                "zstd-19": bl["zstd-19"]["bytes_comp"],
                "xz-9e": bl["xz-9e"]["bytes_comp"],
                "openzl_melhor": it["openzl"][mb]["bytes_comp"] if mb else None,
                "openzl_generico": it["openzl"][mg]["bytes_comp"] if mg else None,
                "openzl_generico_L6": it["openzl"]["generico:serial"]["bytes_comp"],
            },
        }
        linhas_item.append(linha)
        c = por_classe.setdefault(it["classe"], {"teste_bytes": 0, "zstd-3": 0, "zstd-19": 0, "xz-9e": 0, "openzl_melhor": 0, "openzl_generico": 0, "openzl_generico_L6": 0, "arquivos": []})
        c["teste_bytes"] += it["teste_bytes"]
        c["zstd-3"] += linha["bytes_comp"]["zstd-3"]
        c["zstd-19"] += linha["bytes_comp"]["zstd-19"]
        c["xz-9e"] += linha["bytes_comp"]["xz-9e"]
        c["openzl_melhor"] += linha["bytes_comp"]["openzl_melhor"] or 0
        c["openzl_generico"] += linha["bytes_comp"]["openzl_generico"] or 0
        c["openzl_generico_L6"] += linha["bytes_comp"]["openzl_generico_L6"]
        c["arquivos"].append(it["id"])
    classes = []
    for nome, c in sorted(por_classe.items()):
        t = c["teste_bytes"]
        r = {k: t / c[k] for k in ("zstd-3", "zstd-19", "xz-9e", "openzl_melhor", "openzl_generico", "openzl_generico_L6") if c[k]}
        classes.append(
            {
                "classe": nome,
                "arquivos": c["arquivos"],
                "teste_bytes": t,
                "razao": r,
                "ganho_melhor_pct": {
                    "sobre_zstd-3": ganho(r.get("openzl_melhor"), r.get("zstd-3")),
                    "sobre_zstd-19": ganho(r.get("openzl_melhor"), r.get("zstd-19")),
                    "sobre_xz-9e": ganho(r.get("openzl_melhor"), r.get("xz-9e")),
                },
                "ganho_generico_pct": {
                    "sobre_zstd-3": ganho(r.get("openzl_generico"), r.get("zstd-3")),
                    "sobre_zstd-19": ganho(r.get("openzl_generico"), r.get("zstd-19")),
                    "sobre_xz-9e": ganho(r.get("openzl_generico"), r.get("xz-9e")),
                },
            }
        )
    return {"itens": linhas_item, "classes": classes}


def tabela_md(cabecalho, linhas):
    saida = ["| " + " | ".join(cabecalho) + " |", "|" + "|".join(["---"] * len(cabecalho)) + "|"]
    for l in linhas:
        saida.append("| " + " | ".join(str(x) for x in l) + " |")
    return "\n".join(saida)


def cmd_relatorio(a):
    with open(a.razao) as f:
        razao = limpar_velocidades(json.load(f))
    tempo = None
    if a.tempo and os.path.exists(a.tempo):
        with open(a.tempo) as f:
            tempo = json.load(f)
    rel = construir_relatorio(razao, tempo)
    det = None
    if a.determinismo and os.path.exists(a.determinismo):
        with open(a.determinismo) as f:
            det = json.load(f)
    saida_json = {
        "gerado_em": time.strftime("%Y-%m-%d %H:%M:%S"),
        "ambiente": razao.get("ambiente"),
        "protocolo": razao.get("protocolo"),
        "conferencia_dados": razao.get("conferencia_dados"),
        "conferencia_zstd3_vs_medicoes": razao.get("conferencia_zstd3_vs_medicoes"),
        "sinteticos_novos": razao.get("manifesto_fatias_resumo", {}).get("sinteticos_novos"),
        "violacoes_bit_exatidao": razao.get("violacoes_bit_exatidao"),
        "falhas": razao.get("falhas"),
        "tabela_por_arquivo": rel["itens"],
        "tabela_por_classe": rel["classes"],
        "treinos": {it["id"]: it["treinos"] for it in razao["itens"] if it.get("treinos")},
        "determinismo_do_treino": det,
        "referencias_publicadas_rotuladas": REFERENCIAS_PUBLICADAS,
        "tempo": tempo if tempo else "NAO MEDIDO (fase B pendente)",
        "bruto_razao": razao["itens"],
    }
    gravar_json(a.json, saida_json)
    md = []
    md.append("# OpenZL como baseline de compressao (T1.9, fase A: razao e bit-exatidao, sem tempo)")
    md.append("")
    amb = razao.get("ambiente", {})
    md.append("Gerado por `bench/openzl_bench.py relatorio` em %s a partir de `%s`. Imagem `%s`, commit OpenZL `%s`, `%s`." % (
        saida_json["gerado_em"], os.path.basename(a.razao), amb.get("imagem"), amb.get("commit_openzl"), amb.get("zli")))
    md.append("")
    md.append("VELOCIDADE NAO MEDIDA nesta fase (maquina compartilhada). Violacoes de bit-exatidao: %d. Falhas de execucao: %d." % (
        len(razao.get("violacoes_bit_exatidao", [])), len(razao.get("falhas", []))))
    md.append("")
    md.append("## Conferencias")
    md.append("")
    ok_dados = all(c.get("confere_com_medicoes_json") for c in razao.get("conferencia_dados", []) if c.get("presente"))
    md.append("- sha256 dos %d arquivos de entrada contra `medicoes.json`: %s." % (len(razao.get("conferencia_dados", [])), "todos conferem" if ok_dados else "DIVERGENCIA"))
    cz = razao.get("conferencia_zstd3_vs_medicoes", [])
    md.append("- zstd-3 (`compression.zstd`) na fatia de 8 MiB de `medicoes.json`: %d de %d arquivos com bytes comprimidos identicos aos de `medicoes.json`." % (
        sum(1 for x in cz if x["identico"]), len(cz)))
    md.append("")
    md.append("## Por classe (soma dos bytes das fatias de teste; razao = original / comprimido)")
    md.append("")
    linhas = []
    for c in rel["classes"]:
        r = c["razao"]
        linhas.append([
            c["classe"], c["teste_bytes"], fmt(r.get("zstd-3")), fmt(r.get("zstd-19")), fmt(r.get("xz-9e")),
            fmt(r.get("openzl_generico_L6")), fmt(r.get("openzl_generico")), fmt(r.get("openzl_melhor")),
            fmt_pct(c["ganho_melhor_pct"]["sobre_zstd-3"]), fmt_pct(c["ganho_melhor_pct"]["sobre_zstd-19"]), fmt_pct(c["ganho_melhor_pct"]["sobre_xz-9e"]),
        ])
    md.append(tabela_md(["classe", "bytes de teste", "zstd-3", "zstd-19", "xz-9e", "OZL generico L6", "OZL generico (melhor de L6/L9)", "OZL melhor", "ganho vs zstd-3", "vs zstd-19", "vs xz-9e"], linhas))
    md.append("")
    md.append("`OpenZL melhor` e o melhor entre generico, perfil e treinado, escolhido no proprio teste (otimista). Ganho = razao OpenZL / razao do baseline - 1.")
    md.append("")
    md.append("## Por arquivo")
    md.append("")
    linhas = []
    for l in rel["itens"]:
        r = l["razao"]
        linhas.append([
            l["id"], l["classe"], "SIM" if l["sintetico"] else "nao", l["teste_bytes"],
            fmt(r["zstd-1"]), fmt(r["zstd-3"]), fmt(r["zstd-19"]), fmt(r["xz-9e"]),
            fmt(r["openzl_generico_L6"]), fmt(r["openzl_generico"]), fmt(r["openzl_perfil"]), fmt(r["openzl_treinado"]),
            l["melhor_configuracao"],
            fmt_pct(l["ganho_pct"]["sobre_zstd-3"]), fmt_pct(l["ganho_pct"]["sobre_zstd-19"]), fmt_pct(l["ganho_pct"]["sobre_xz-9e"]),
        ])
    md.append(tabela_md(
        ["arquivo", "classe", "sintetico", "bytes teste", "zstd-1", "zstd-3", "zstd-19", "xz-9e", "OZL gen. L6", "OZL gen. melhor L6/L9", "OZL perfil", "OZL treinado", "melhor OZL", "vs zstd-3", "vs zstd-19", "vs xz-9e"],
        linhas))
    md.append("")
    md.append("## Onde o OpenZL perde ou nao usa codecs estruturais")
    md.append("")
    linhas = []
    for l in rel["itens"]:
        r = l["razao"]
        perde = [n for n in ("zstd-3", "zstd-19", "xz-9e") if r["openzl_melhor"] is not None and r["openzl_melhor"] < r[n]]
        linhas.append([l["id"], ", ".join(perde) if perde else "-", ", ".join(l["sem_codec_estrutural"]) or "-", ", ".join(l["estrito_falhou"]) or "-"])
    md.append(tabela_md(["arquivo", "melhor OpenZL perde para", "configs sem nenhum codec estrutural no grafo resolvido (trace)", "configs em que --strict falhou"], linhas))
    md.append("")
    md.append("## Treinos")
    md.append("")
    linhas = []
    for it in razao["itens"]:
        for k, v in it.get("treinos", {}).items():
            linhas.append([it["id"], k, v["rc"], v["tempo_relogio_s"], "%d%s" % (v["max_time_secs"], " (atingido)" if v.get("atingiu_limite_de_tempo") else ""), v["tamanho_compressor_bytes"], fmt(v.get("ganho_in_sample_pct"), 2)])
    md.append(tabela_md(["arquivo", "treino", "rc", "relogio (s)", "limite (s)", "bytes do compressor", "ganho no treino (%, in-sample, reportado pelo trainer)"], linhas))
    md.append("")
    if det:
        md.append("### Determinismo do treino")
        md.append("")
        linhas = [[r["sha256_compressor"][:16] if r["sha256_compressor"] else "-", r["tamanho_compressor_bytes"], r["bytes_comp_teste"], fmt(r["razao_teste"], 4)] for r in det["rodadas"]]
        md.append("`%s` perfil `%s`, rodadas repetidas sobre as mesmas amostras:" % (det["id"], det["perfil"]))
        md.append("")
        md.append(tabela_md(["sha256 do compressor", "bytes do compressor", "bytes comprimidos no teste", "razao no teste"], linhas))
        md.append("")
    md.append("## Referencias publicadas (apenas rotuladas; nao usadas para concluir)")
    md.append("")
    for ref in REFERENCIAS_PUBLICADAS:
        md.append("- %s | dado: %s | %s | %s" % (ref["fonte"], ref["dado"], ", ".join("%s=%s" % (k, v) for k, v in ref.items() if k not in ("fonte", "dado", "obs")), ref["obs"]))
    md.append("")
    md.append("## Tempo")
    md.append("")
    if tempo:
        linhas = []
        for it in tempo["itens"]:
            for rot, v in {**it["baselines"], **it["openzl"]}.items():
                linhas.append([it["id"], rot, fmt(v["comp_MBps_mediana"], 1), fmt(v["decomp_MBps_mediana"], 1)])
        md.append("CPU %s, %d repeticoes, mediana." % (tempo["cpu"], tempo["repeticoes"]))
        md.append("")
        md.append(tabela_md(["arquivo", "config", "compressao MB/s", "descompressao MB/s"], linhas))
    else:
        md.append("NAO MEDIDO: fase B pendente, depende de janela exclusiva da maquina (`openzl_bench.py tempo`).")
    md.append("")
    with open(a.md, "w") as f:
        f.write("\n".join(md))
    print("escrito", a.md, a.json)


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("preparar")
    p.add_argument("--dados", required=True)
    p.add_argument("--fatias", required=True)
    p.add_argument("--medicoes", default="bench/resultados/medicoes.json")
    p.add_argument("--semente", type=int, default=SEMENTE_PADRAO)
    p.set_defaults(fn=cmd_preparar)
    p = sub.add_parser("razao")
    p.add_argument("--dados", required=True)
    p.add_argument("--fatias", required=True)
    p.add_argument("--medicoes", default="bench/resultados/medicoes.json")
    p.add_argument("--saida", default="bench/resultados/openzl_razao.json")
    p.add_argument("--treino-max-s", type=int, default=360)
    p.add_argument("--threads", type=int, default=16)
    p.add_argument("--sem-treino", action="store_true")
    p.add_argument("--retomar", action="store_true")
    p.add_argument("--somente", nargs="*")
    p.set_defaults(fn=cmd_razao)
    p = sub.add_parser("determinismo-treino")
    p.add_argument("--fatias", required=True)
    p.add_argument("--id", required=True)
    p.add_argument("--perfil", required=True)
    p.add_argument("--rodadas", type=int, default=2)
    p.add_argument("--treino-max-s", type=int, default=360)
    p.add_argument("--threads", type=int, default=16)
    p.add_argument("--saida", default="bench/resultados/openzl_determinismo_treino.json")
    p.set_defaults(fn=cmd_determinismo_treino)
    p = sub.add_parser("tempo")
    p.add_argument("--fatias", required=True)
    p.add_argument("--saida", default="bench/resultados/openzl_tempo.json")
    p.add_argument("--cpu", default="0")
    p.add_argument("--repeticoes", type=int, default=3)
    p.add_argument("--somente", nargs="*")
    p.add_argument("--carga-maxima", type=float, default=1.0)
    p.add_argument("--confirmo-janela-exclusiva", action="store_true")
    p.add_argument("--dry-run", action="store_true")
    p.set_defaults(fn=cmd_tempo)
    p = sub.add_parser("relatorio")
    p.add_argument("--razao", default="bench/resultados/openzl_razao.json")
    p.add_argument("--tempo", default="bench/resultados/openzl_tempo.json")
    p.add_argument("--determinismo", default="bench/resultados/openzl_determinismo_treino.json")
    p.add_argument("--md", default="bench/resultados/openzl.md")
    p.add_argument("--json", default="bench/resultados/openzl.json")
    p.set_defaults(fn=cmd_relatorio)
    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
