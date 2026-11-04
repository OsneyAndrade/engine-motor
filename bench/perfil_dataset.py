import argparse
import bz2
import errno
import hashlib
import importlib.util
import json
import lzma
import math
import multiprocessing as mp
import os
import re
import stat
import sys
import time
from collections import Counter, defaultdict

import numpy as np
from compression import zstd

sys.dont_write_bytecode = True

VERSAO = "1.0"
KIB = 1 << 10
MIB = 1 << 20
BASE = os.path.dirname(os.path.abspath(__file__))
MAX_PROCESSOS = 5

CLASSES = [
    "texto",
    "codigo",
    "csv_tabular",
    "json_ndjson",
    "log",
    "xml_html",
    "sequencia",
    "binario",
    "banco_de_dados",
    "cientifico_numerico",
    "imagem_audio_video",
    "container_comprimido",
    "criptografado_aleatorio",
    "vazio",
]
CLASSES_INCOMPRESSIVEIS = ("imagem_audio_video", "container_comprimido", "criptografado_aleatorio")
CODECS_RAPIDOS = ["zstd-1", "zstd-3", "zstd-9"]
CODECS_LENTOS = ["zstd-19", "xz-9e", "bzip2-9"]
CODECS = CODECS_RAPIDOS + CODECS_LENTOS
TRANSFORMS = [("d1", "delta", 1), ("d2", "delta", 2), ("d4", "delta", 4), ("d8", "delta", 8), ("b4", "bsplit", 4), ("b8", "bsplit", 8)]
CLASSES_TRANSFORM = ("cientifico_numerico",)
LIMIAR_ENTROPIA_ALEATORIO = 7.9
LIMIAR_BLOCO_INCOMPRESSIVEL = 0.03
JANELA_CLASSIFICACAO = 16 * KIB
JANELA_HASH = 64 * KIB
LIMIAR_GANHO_DELTA_BITS = 0.75
TAMANHO_BLOCO = MIB

PROXY_MEDICOES = {
    "texto": ("texto", False),
    "codigo": ("codigo", False),
    "csv_tabular": ("texto", False),
    "json_ndjson": ("xml", False),
    "log": ("sint_logs", True),
    "xml_html": ("xml", False),
    "sequencia": ("dna", False),
    "binario": ("binario", False),
    "banco_de_dados": ("banco_de_dados", False),
    "cientifico_numerico": ("cientifico", False),
    "imagem_audio_video": ("sint_ja_comprimido", True),
    "container_comprimido": ("sint_ja_comprimido", True),
    "criptografado_aleatorio": ("sint_incompressivel", True),
}

EXT_CODIGO = set(
    "py js mjs cjs ts tsx jsx c h cc cpp cxx hpp hh rs go java kt kts scala swift rb php pl pm lua sh bash zsh ps1 bat cs fs vb sql r jl m mm dart hs ml clj ex exs erl cu cl glsl vhd vhdl v sv asm s css scss less proto gradle cmake mk toml yaml yml ini cfg conf tf".split()
)
EXT_TEXTO = set("txt md rst tex rtf adoc org nfo text".split())
EXT_CSV = set("csv tsv psv tab".split())
EXT_JSON = set("json jsonl ndjson geojson json5".split())
EXT_LOG = set("log out err trace".split())
EXT_XML = set("xml html htm xhtml svg xsd xsl xslt rss atom kml plist".split())
EXT_SEQ = set("fa fasta fna faa fastq fq gb gbk".split())
EXT_BANCO = set("db sqlite sqlite3 db3 mdb accdb ldb sst ibd myd myi frm dbf mdf ldf rdb lmdb".split())
EXT_CONTAINER = set(
    "zip gz tgz bz2 tbz2 xz txz zst zstd 7z rar lz4 lz lzma z cab jar war apk whl parquet orc avro arrow feather docx xlsx pptx odt ods odp epub syntra snappy br deb rpm".split()
)
EXT_MEDIA = set(
    "jpg jpeg png gif webp avif heic heif jp2 mp3 aac m4a ogg oga opus flac wma mp4 m4v mov mkv webm avi wmv flv mpg mpeg 3gp".split()
)
EXT_CIENT = set("npy h5 hdf5 hdf nc cdf fits fit mat dcm grib grb tif tiff bmp wav raw".split())
EXT_CRIPTO = set("enc gpg pgp age crypt aes luks kdbx".split())
EXT_BINARIO = set("exe dll so o a elf class pyc wasm dylib obj lib img iso vmdk qcow2 vdi".split())

ASSINATURAS = [
    (0, b"PK\x03\x04", "zip", "container_comprimido"),
    (0, b"PK\x05\x06", "zip", "container_comprimido"),
    (0, b"\x1f\x8b", "gzip", "container_comprimido"),
    (0, b"\xfd7zXZ\x00", "xz", "container_comprimido"),
    (0, b"\x28\xb5\x2f\xfd", "zstd", "container_comprimido"),
    (0, b"7z\xbc\xaf\x27\x1c", "7z", "container_comprimido"),
    (0, b"Rar!\x1a\x07", "rar", "container_comprimido"),
    (0, b"\x04\x22\x4d\x18", "lz4", "container_comprimido"),
    (0, b"PAR1", "parquet", "container_comprimido"),
    (0, b"ORC", "orc", "container_comprimido"),
    (0, b"ARROW1", "arrow", "container_comprimido"),
    (0, b"Obj\x01", "avro", "container_comprimido"),
    (0, b"\x89PNG\r\n\x1a\n", "png", "imagem_audio_video"),
    (0, b"\xff\xd8\xff", "jpeg", "imagem_audio_video"),
    (0, b"GIF87a", "gif", "imagem_audio_video"),
    (0, b"GIF89a", "gif", "imagem_audio_video"),
    (0, b"\x00\x00\x00\x0cjP  ", "jpeg2000", "imagem_audio_video"),
    (0, b"fLaC", "flac", "imagem_audio_video"),
    (0, b"OggS", "ogg", "imagem_audio_video"),
    (0, b"ID3", "mp3", "imagem_audio_video"),
    (0, b"\x1a\x45\xdf\xa3", "matroska", "imagem_audio_video"),
    (0, b"\x00\x00\x01\xba", "mpeg", "imagem_audio_video"),
    (0, b"\x00\x00\x01\xb3", "mpeg", "imagem_audio_video"),
    (0, b"FLV\x01", "flv", "imagem_audio_video"),
    (0, b"\x30\x26\xb2\x75\x8e\x66\xcf\x11", "asf", "imagem_audio_video"),
    (0, b"\x7fELF", "elf", "binario"),
    (0, b"\xfe\xed\xfa\xce", "macho", "binario"),
    (0, b"\xfe\xed\xfa\xcf", "macho", "binario"),
    (0, b"\xce\xfa\xed\xfe", "macho", "binario"),
    (0, b"\xcf\xfa\xed\xfe", "macho", "binario"),
    (0, b"\xca\xfe\xba\xbe", "java_class_ou_macho", "binario"),
    (0, b"\x00asm", "wasm", "binario"),
    (0, b"SQLite format 3\x00", "sqlite", "banco_de_dados"),
    (0, b"\x89HDF\r\n\x1a\n", "hdf5", "cientifico_numerico"),
    (0, b"CDF\x01", "netcdf", "cientifico_numerico"),
    (0, b"CDF\x02", "netcdf", "cientifico_numerico"),
    (0, b"SIMPLE  =", "fits", "cientifico_numerico"),
    (0, b"\x93NUMPY", "npy", "cientifico_numerico"),
    (0, b"MATLAB 5.0", "mat", "cientifico_numerico"),
    (128, b"DICM", "dicom", "cientifico_numerico"),
    (0, b"Salted__", "openssl_enc", "criptografado_aleatorio"),
    (0, b"LUKS\xba\xbe", "luks", "criptografado_aleatorio"),
    (0, b"age-encryption.org/v1", "age", "criptografado_aleatorio"),
    (0, b"-----BEGIN PGP MESSAGE-----", "pgp_armor", "criptografado_aleatorio"),
]

RE_TS_LOG = re.compile(
    rb"^(\[?\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}|\[?\w{3}\s+\d{1,2}\s\d{2}:\d{2}:\d{2}|\d{1,3}(\.\d{1,3}){3}\s.*\[\d{2}/\w{3}/\d{4}|\[?\d{2}/\d{2}/\d{4}[ :]|\d{2}:\d{2}:\d{2}[ .,])"
)
RE_NIVEL_LOG = re.compile(rb"\b(INFO|WARN(ING)?|ERROR|DEBUG|TRACE|FATAL|NOTICE)\b")
RE_MARCADORES_CODIGO = re.compile(
    rb"(^|\n)[ \t]*(#include|#define|#ifdef|import |from \S+ import |def |class |function |package |public |private |static |const |let |var |fn |func |using |namespace |struct |typedef )"
)
RE_CONTROLE_CODIGO = re.compile(rb"\b(if|for|while|switch|return)\s*\(|\)\s*\{|;\s*(\n|$)")


def extensao(nome):
    s = nome.decode("utf-8", "surrogateescape").lower()
    i = s.rfind(".")
    if i <= 0 or i == len(s) - 1:
        return ""
    e = s[i + 1 :]
    if len(e) > 10 or not re.fullmatch(r"[a-z0-9_+\-]+", e):
        return "?"
    if e.isdigit():
        return "#"
    return e


def rotulo_extensao(e):
    return {"": "(sem extensao)", "?": "(irregular)", "#": "(numerica)"}.get(e, e)


def entropia_contagem(c, n):
    if n <= 0:
        return 0.0, 0.0
    nz = c[c > 0].astype(np.float64)
    p = nz / n
    h = float(-(p * np.log2(p)).sum())
    return h, min(8.0, h + (len(nz) - 1) / (2.0 * n * math.log(2)))


def entropia_bytes(b):
    n = len(b)
    if n == 0:
        return 0.0, 0.0
    return entropia_contagem(np.bincount(np.frombuffer(b, dtype=np.uint8), minlength=256), n)


def aplicar_delta(b, w):
    n = len(b) // w
    if n == 0:
        return bytes(b)
    a = np.frombuffer(b, dtype=np.dtype("<u%d" % w), count=n)
    d = np.empty_like(a)
    d[0] = a[0]
    np.subtract(a[1:], a[:-1], out=d[1:])
    return d.tobytes() + bytes(b[n * w :])


def aplicar_bsplit(b, w):
    n = len(b) // w
    if n == 0:
        return bytes(b)
    a = np.frombuffer(b, dtype=np.uint8, count=n * w).reshape(n, w)
    return np.ascontiguousarray(a.T).tobytes() + bytes(b[n * w :])


def aplicar_transform(b, tipo, w):
    return aplicar_delta(b, w) if tipo == "delta" else aplicar_bsplit(b, w)


def entropia_plano_medio(b, w):
    n = len(b) // w
    if n < 256:
        return None
    a = np.frombuffer(b, dtype=np.uint8, count=n * w).reshape(n, w)
    return float(np.mean([entropia_bytes(a[:, i].tobytes())[0] for i in range(w)]))


def entropia_transform_minima(b):
    melhor = None
    for w in (1, 2, 4, 8):
        n = len(b) // w
        if n < 256:
            continue
        d = aplicar_delta(b, w)
        cand = [("delta", w, entropia_bytes(d)[0])]
        if w > 1:
            ep = entropia_plano_medio(d, w)
            if ep is not None:
                cand.append(("delta+plano", w, ep))
            ep = entropia_plano_medio(b, w)
            if ep is not None:
                cand.append(("plano", w, ep))
        for c in cand:
            if melhor is None or c[2] < melhor[2]:
                melhor = c
    return melhor


def utf8_valido(b):
    try:
        b.decode("utf-8")
        return True
    except UnicodeDecodeError as e:
        return e.start >= len(b) - 3 and e.reason == "unexpected end of data"


def detectar_assinatura(head, ext, tam):
    for off, mag, rot, cls in ASSINATURAS:
        if head[off : off + len(mag)] == mag:
            return rot, cls
    if head[:3] == b"BZh" and len(head) > 3 and head[3:4] in b"123456789":
        return "bzip2", "container_comprimido"
    if head[257:262] == b"ustar":
        return "tar", None
    if head[:5] == b"%PDF-":
        return "pdf", None
    if head[:4] == b"RIFF" and len(head) >= 12:
        t = head[8:12]
        if t == b"WEBP":
            return "webp", "imagem_audio_video"
        if t == b"AVI ":
            return "avi", "imagem_audio_video"
        if t == b"WAVE":
            return "wav", None
    if head[4:8] == b"ftyp":
        marca = head[8:12]
        if marca in (b"avif", b"avis", b"heic", b"heix", b"mif1", b"msf1", b"heim", b"heis"):
            return "heif", "imagem_audio_video"
        return "iso_bmff", "imagem_audio_video"
    if head[:4] in (b"II*\x00", b"MM\x00*"):
        return "tiff", None
    if head[:2] == b"BM" and ext == "bmp":
        return "bmp", None
    if head[:2] == b"MZ" and len(head) >= 64:
        pe = int.from_bytes(head[60:64], "little")
        if 64 <= pe < len(head) - 4 and head[pe : pe + 4] == b"PE\x00\x00":
            return "pe", "binario"
    if ext in ("mp3", "aac", "mp2") and len(head) > 2 and head[0] == 0xFF and (head[1] & 0xE0) == 0xE0:
        return "mpeg_audio", "imagem_audio_video"
    return None, None


def contar_linhas_validas(head, completo):
    linhas = head.split(b"\n")
    if not completo and len(linhas) > 1:
        linhas = linhas[:-1]
    return [l.rstrip(b"\r") for l in linhas if l.strip()]


def parece_json(head, tam, ext):
    topo = head.lstrip(b"\xef\xbb\xbf \t\r\n")
    if not topo[:1] in (b"{", b"["):
        return None
    completo = len(head) >= tam
    linhas = contar_linhas_validas(head, completo)
    ok = 0
    for l in linhas[:5]:
        if len(l) > 64 * KIB:
            break
        try:
            json.loads(l)
            ok += 1
        except (ValueError, RecursionError):
            break
    if len(linhas) >= 3 and ok >= min(3, len(linhas)) and all(l[:1] in (b"{", b"[") for l in linhas[: min(len(linhas), 20)]):
        return "ndjson"
    if completo and len(head) <= JANELA_CLASSIFICACAO:
        try:
            json.loads(head)
            return "json"
        except (ValueError, RecursionError):
            pass
    asp = head.count(b'"')
    if ext in EXT_JSON or (asp >= 0.02 * len(head) and head.count(b'":') >= 5 and head.count(b"{") + head.count(b"[") >= 2):
        return "json"
    return None


def parece_xml(head, ext):
    topo = head.lstrip(b"\xef\xbb\xbf \t\r\n")
    baixo = topo[:512].lower()
    if baixo.startswith((b"<?xml", b"<!doctype html", b"<html", b"<svg", b"<!--")):
        return True
    if topo[:1] == b"<" and head.count(b"</") >= 3 and head.count(b"<") >= 0.01 * len(head):
        return True
    return head.count(b"</") >= 10 and head.count(b"<") >= 0.02 * len(head)


def parece_csv(head, tam):
    completo = len(head) >= tam
    linhas = contar_linhas_validas(head, completo)
    if len(linhas) < 5:
        return None
    linhas = linhas[:60]
    if max(len(l) for l in linhas) > 8 * KIB:
        return None
    melhor = None
    for d, min_colunas in ((b",", 3), (b"\t", 2), (b";", 3), (b"|", 3)):
        cont = [l.count(d) + 1 for l in linhas]
        moda, freq = Counter(cont).most_common(1)[0]
        if moda >= min_colunas and freq / len(linhas) >= 0.8:
            if melhor is None or freq > melhor[1]:
                melhor = (d, freq, moda)
    return melhor


def parece_log(head, tam):
    completo = len(head) >= tam
    linhas = contar_linhas_validas(head, completo)
    if len(linhas) < 4:
        return False
    linhas = linhas[:80]
    ts = sum(1 for l in linhas if RE_TS_LOG.match(l))
    nivel = sum(1 for l in linhas if RE_NIVEL_LOG.search(l))
    return ts / len(linhas) >= 0.6 or (nivel / len(linhas) >= 0.5 and ts / len(linhas) >= 0.2)


def parece_sequencia(head):
    corpo = b"".join(l for l in head.split(b"\n") if l[:1] not in (b">", b"@", b"+", b";"))
    corpo = corpo.replace(b"\r", b"")
    if len(corpo) < 256:
        return False
    cont = np.bincount(np.frombuffer(corpo, dtype=np.uint8), minlength=256)
    ok = sum(int(cont[ord(c)]) for c in "ACGTNacgtn")
    return ok / len(corpo) >= 0.95


def parece_codigo(head):
    if head[:2] == b"#!":
        return True
    amostra = head[: 32 * KIB]
    marcadores = {m.group(2).strip() for m in RE_MARCADORES_CODIGO.finditer(amostra)}
    controle = len(RE_CONTROLE_CODIGO.findall(amostra))
    linhas = max(1, amostra.count(b"\n"))
    return len(marcadores) >= 3 or (len(marcadores) >= 2 and controle / linhas >= 0.15) or controle / linhas >= 0.35


def subclasse_texto(head, ext, tam):
    js = parece_json(head, tam, ext)
    if js is not None:
        return "json_ndjson", "conteudo_" + js
    if ext in EXT_JSON:
        return "json_ndjson", "extensao"
    if ext in EXT_XML or parece_xml(head, ext):
        return "xml_html", "extensao" if ext in EXT_XML else "conteudo"
    if ext in EXT_LOG or parece_log(head, tam):
        return "log", "extensao" if ext in EXT_LOG else "conteudo"
    if ext in EXT_CSV:
        return "csv_tabular", "extensao"
    if ext in EXT_SEQ or parece_sequencia(head):
        return "sequencia", "extensao" if ext in EXT_SEQ else "conteudo"
    if ext in EXT_CODIGO:
        return "codigo", "extensao"
    if ext not in EXT_TEXTO and parece_csv(head, tam):
        return "csv_tabular", "conteudo"
    if parece_codigo(head):
        return "codigo", "conteudo"
    return "texto", "conteudo"


def classificar_amostra(head, amostra, ext, tam):
    if tam == 0:
        return "vazio", "tamanho_zero", {}
    rot, cls = detectar_assinatura(head, ext, tam)
    n = len(amostra)
    h, hmm = entropia_bytes(amostra)
    nul = amostra.count(0) / n if n else 0.0
    nao_nul = n - amostra.count(0)
    cont = np.bincount(np.frombuffer(amostra, dtype=np.uint8), minlength=256) if n else np.zeros(256, dtype=np.int64)
    imp = int(cont[9] + cont[10] + cont[13] + cont[32:127].sum() + cont[128:].sum())
    frac_imp = imp / n if n else 0.0
    frac_imp_nn = imp / nao_nul if nao_nul else 0.0
    evid = {"entropia": round(h, 4), "entropia_corrigida": round(hmm, 4), "imprimiveis": round(frac_imp, 4), "nul": round(nul, 4), "assinatura": rot}
    if cls is not None:
        return cls, "assinatura:" + rot, evid
    if head[:2] in (b"\xff\xfe", b"\xfe\xff") and frac_imp_nn > 0.95:
        return "texto", "bom_utf16", evid
    if rot == "tar":
        if frac_imp_nn < 0.95:
            return "binario", "assinatura:tar_com_conteudo_binario", evid
        c, m = subclasse_texto(amostra.replace(b"\x00", b""), "", tam << 8)
        return c, "assinatura:tar+" + m, evid
    if rot == "pdf":
        if hmm >= 7.3:
            return "container_comprimido", "assinatura:pdf_entropia_alta", evid
        if frac_imp >= 0.88:
            return "texto", "assinatura:pdf_texto", evid
        return "binario", "assinatura:pdf_binario", evid
    if rot in ("tiff", "bmp", "wav"):
        if hmm >= 7.3:
            return "imagem_audio_video", "assinatura:%s_entropia_alta" % rot, evid
        return "cientifico_numerico", "assinatura:%s_bruto" % rot, evid
    alto = int(cont[128:].sum()) / n if n else 0.0
    texto = frac_imp >= 0.88 and nul < 0.01 and hmm < 7.0 and (alto <= 0.30 or utf8_valido(head))
    if n >= 1024 and hmm >= LIMIAR_ENTROPIA_ALEATORIO and not texto:
        if ext in EXT_CONTAINER:
            return "container_comprimido", "extensao_sem_assinatura", evid
        if ext in EXT_MEDIA:
            return "imagem_audio_video", "extensao_sem_assinatura", evid
        return "criptografado_aleatorio", "entropia>=%.1f_sem_assinatura" % LIMIAR_ENTROPIA_ALEATORIO, evid
    if texto:
        c, m = subclasse_texto(head, ext, tam)
        return c, m, evid
    if ext in EXT_CONTAINER and hmm >= 7.3:
        return "container_comprimido", "extensao", evid
    if ext in EXT_MEDIA and hmm >= 7.3:
        return "imagem_audio_video", "extensao", evid
    if ext in EXT_CRIPTO:
        return "criptografado_aleatorio", "extensao", evid
    if ext in EXT_BANCO:
        return "banco_de_dados", "extensao", evid
    if ext in EXT_BINARIO:
        return "binario", "extensao", evid
    if ext in EXT_CIENT:
        return "cientifico_numerico", "extensao", evid
    dm = entropia_transform_minima(amostra) if n >= 4096 else None
    if dm is not None:
        evid["entropia_transform_minima"] = round(dm[2], 4)
        evid["transform_minimo"] = "%s%d" % (dm[0], dm[1])
        if h - dm[2] >= LIMIAR_GANHO_DELTA_BITS and dm[2] < 7.0:
            return "cientifico_numerico", "entropia_dos_deltas", evid
    return "binario", "conteudo_binario", evid


FLAGS_DIR = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW | os.O_CLOEXEC
FLAGS_ARQ = os.O_RDONLY | os.O_NOFOLLOW | os.O_CLOEXEC | os.O_NONBLOCK
NOATIME = getattr(os, "O_NOATIME", 0)
MOTIVOS_ERRNO = {
    errno.EACCES: "sem_permissao_leitura",
    errno.EPERM: "sem_permissao_leitura",
    errno.ENOENT: "removido_durante_leitura",
    errno.ELOOP: "link_simbolico",
    errno.ENXIO: "nao_regular",
    errno.EISDIR: "nao_regular",
    errno.ENOTDIR: "nao_regular",
    errno.EIO: "erro_io",
    errno.EMFILE: "muitos_arquivos_abertos",
    errno.ENFILE: "muitos_arquivos_abertos",
}


class Falha(Exception):
    def __init__(self, motivo):
        Exception.__init__(self, motivo)
        self.motivo = motivo


def motivo_de(e):
    return MOTIVOS_ERRNO.get(e.errno, "erro_" + errno.errorcode.get(e.errno, str(e.errno)).lower())


class Arq:
    __slots__ = (
        "i",
        "dir",
        "nome",
        "tam",
        "mtime",
        "dev",
        "ino",
        "ext",
        "classe",
        "motivo_classe",
        "status",
        "h_amostra",
        "h_completo",
        "lido_classif",
        "lido_dedup",
        "lido_amostra",
        "lido_delta",
        "redundante",
        "familia",
        "base",
    )

    def __init__(self, i, did, nome, tam, mtime, dev, ino):
        self.i = i
        self.dir = did
        self.nome = nome
        self.tam = tam
        self.mtime = mtime
        self.dev = dev
        self.ino = ino
        self.ext = extensao(nome)
        self.classe = None
        self.motivo_classe = None
        self.status = None
        self.h_amostra = None
        self.h_completo = None
        self.lido_classif = 0
        self.lido_dedup = 0
        self.lido_amostra = 0
        self.lido_delta = 0
        self.redundante = False
        self.familia = None
        self.base = False


class Anon:
    def __init__(self, revelar=False, sal=None):
        self.revelar = revelar
        self.sal = sal if sal is not None else os.urandom(16)

    def id(self, tag, dado):
        return hashlib.blake2b(tag.encode() + b"\x00" + dado, key=self.sal, digest_size=8).hexdigest()

    def nome(self, rel):
        return rel.decode("utf-8", "backslashreplace") if self.revelar else None


class Varredura:
    def __init__(self):
        self.arquivos = []
        self.dirs = [(-1, b"")]
        self.ignorados = Counter()
        self.ign_lista = []
        self.truncada = False
        self.nomes_invalidos = 0
        self.dirs_inacessiveis = 0
        self.raiz_real = None

    def caminho(self, did, nome=None):
        partes = []
        while did > 0:
            pai, n = self.dirs[did]
            partes.append(n)
            did = pai
        partes.reverse()
        if nome is not None:
            partes.append(nome)
        return b"/".join(partes)


def abrir_raiz(raiz):
    real = os.path.realpath(raiz)
    fd = os.open(real, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    return fd, real


def listar_dir(fd):
    saida = []
    with os.scandir(fd) as it:
        for e in it:
            nome = os.fsencode(e.name)
            st = None
            try:
                if e.is_symlink():
                    tipo = "l"
                elif e.is_dir(follow_symlinks=False):
                    tipo = "d"
                    st = e.stat(follow_symlinks=False)
                elif e.is_file(follow_symlinks=False):
                    tipo = "f"
                    st = e.stat(follow_symlinks=False)
                else:
                    tipo = "o"
            except OSError:
                tipo = "e"
            saida.append((nome, tipo, st))
    saida.sort(key=lambda x: x[0])
    return saida


def varrer_arvore(raiz_fd, max_arquivos=0, prof_max=64, mesmo_dispositivo=True):
    v = Varredura()
    st_raiz = os.fstat(raiz_fd)
    vistos = set()
    pilha = []

    def ignorar(motivo, did, nome, tam=None):
        v.ignorados[motivo] += 1
        v.ign_lista.append((motivo, v.caminho(did, nome), tam))

    def entrar(did, fd, prof):
        try:
            ents = listar_dir(fd)
        except OSError as e:
            v.dirs_inacessiveis += 1
            ignorar("sem_permissao_diretorio" if e.errno in (errno.EACCES, errno.EPERM) else motivo_de(e), v.dirs[did][0], v.dirs[did][1])
            os.close(fd)
            return
        subdirs = []
        for nome, tipo, st in ents:
            if tipo == "l":
                ignorar("link_simbolico", did, nome)
            elif tipo == "o":
                ignorar("nao_regular", did, nome)
            elif tipo == "e":
                ignorar("erro_stat", did, nome)
            elif tipo == "d":
                if prof + 1 > prof_max:
                    ignorar("profundidade_maxima", did, nome)
                elif mesmo_dispositivo and st.st_dev != st_raiz.st_dev:
                    ignorar("outro_dispositivo", did, nome)
                else:
                    subdirs.append((nome, st))
            else:
                if max_arquivos and len(v.arquivos) >= max_arquivos:
                    v.truncada = True
                    continue
                if st.st_nlink > 1:
                    chave = (st.st_dev, st.st_ino)
                    if chave in vistos:
                        ignorar("hardlink_repetido", did, nome, st.st_size)
                        continue
                    vistos.add(chave)
                try:
                    nome.decode("utf-8")
                except UnicodeDecodeError:
                    v.nomes_invalidos += 1
                v.arquivos.append(Arq(len(v.arquivos), did, nome, st.st_size, st.st_mtime_ns, st.st_dev, st.st_ino))
        pilha.append([did, fd, prof, iter(subdirs)])

    entrar(0, os.dup(raiz_fd), 0)
    while pilha:
        fr = pilha[-1]
        prox = next(fr[3], None)
        if prox is None:
            os.close(fr[1])
            pilha.pop()
            continue
        nome, st = prox
        try:
            fdc = os.open(nome, FLAGS_DIR, dir_fd=fr[1])
        except OSError as e:
            v.dirs_inacessiveis += 1
            ignorar("sem_permissao_diretorio" if e.errno in (errno.EACCES, errno.EPERM) else motivo_de(e), fr[0], nome)
            continue
        did = len(v.dirs)
        v.dirs.append((fr[0], nome))
        entrar(did, fdc, fr[2] + 1)
    return v


class Leitor:
    def __init__(self, raiz_fd, dirs):
        self.raiz_fd = raiz_fd
        self.dirs = dirs
        self.cache = {}
        self.noatime_negados = 0

    def fd_dir(self, did):
        if did == 0:
            return self.raiz_fd
        fd = self.cache.get(did)
        if fd is not None:
            return fd
        pai, nome = self.dirs[did]
        fdp = self.fd_dir(pai)
        try:
            fd = os.open(nome, FLAGS_DIR, dir_fd=fdp)
        except OSError as e:
            raise Falha("sem_permissao_diretorio" if e.errno in (errno.EACCES, errno.EPERM) else motivo_de(e))
        self.cache[did] = fd
        if len(self.cache) > 48:
            antigo = next(iter(self.cache))
            os.close(self.cache.pop(antigo))
        return fd

    def abrir(self, arq):
        fdd = self.fd_dir(arq.dir)
        try:
            try:
                fd = os.open(arq.nome, FLAGS_ARQ | NOATIME, dir_fd=fdd)
            except OSError as e:
                if e.errno != errno.EPERM or not NOATIME:
                    raise
                self.noatime_negados += 1
                fd = os.open(arq.nome, FLAGS_ARQ, dir_fd=fdd)
        except OSError as e:
            raise Falha(motivo_de(e))
        st = os.fstat(fd)
        if not stat.S_ISREG(st.st_mode):
            os.close(fd)
            raise Falha("nao_regular")
        if st.st_dev != arq.dev or st.st_ino != arq.ino:
            os.close(fd)
            raise Falha("substituido_durante_leitura")
        return fd, st

    def ler(self, fd, off, n):
        partes = []
        total = 0
        while total < n:
            try:
                b = os.pread(fd, n - total, off + total)
            except OSError as e:
                raise Falha(motivo_de(e))
            if not b:
                break
            partes.append(b)
            total += len(b)
        return partes[0] if len(partes) == 1 else b"".join(partes)

    def fechar_tudo(self):
        for fd in self.cache.values():
            try:
                os.close(fd)
            except OSError:
                pass
        self.cache.clear()


_G = {}


def leitor_local():
    pid = os.getpid()
    L = _G.get("leitor")
    if L is None or _G.get("leitor_pid") != pid:
        L = Leitor(_G["raiz_fd"], _G["dirs"])
        _G["leitor"] = L
        _G["leitor_pid"] = pid
    return L


def executar(func, tarefas, processos, chunk=1):
    tarefas = list(tarefas)
    if processos <= 1 or len(tarefas) <= 1:
        for t in tarefas:
            yield func(t)
        return
    ctx = mp.get_context("fork")
    with ctx.Pool(min(processos, MAX_PROCESSOS)) as pool:
        for r in pool.imap_unordered(func, tarefas, chunksize=chunk):
            yield r


def lotes(seq, n):
    seq = list(seq)
    return [seq[i : i + n] for i in range(0, len(seq), n)]


def janelas_arquivo(L, fd, tam, completa):
    jan = JANELA_CLASSIFICACAO if completa else 8 * KIB
    head = L.ler(fd, 0, min(tam, jan))
    lido = len(head)
    partes = [head]
    if completa and tam > MIB:
        for off in ((tam - jan) // 2, tam - jan):
            b = L.ler(fd, off, jan)
            lido += len(b)
            partes.append(b)
    return head, b"".join(partes), lido


def tarefa_classificar(lote):
    L = leitor_local()
    arqs = _G["arqs"]
    completa = _G["classif_completa"]
    saida = []
    for i in lote:
        a = arqs[i]
        if a.tam == 0:
            saida.append((i, "vazio", "tamanho_zero", 0, None))
            continue
        try:
            fd, st = L.abrir(a)
        except Falha as f:
            saida.append((i, None, None, 0, f.motivo))
            continue
        try:
            head, amostra, lido = janelas_arquivo(L, fd, st.st_size, completa)
            status = "alterado_durante_leitura" if (st.st_size != a.tam or st.st_mtime_ns != a.mtime) else None
            if not head:
                saida.append((i, "vazio", "lido_vazio", lido, status))
            else:
                c, m, ev = classificar_amostra(head, amostra, a.ext, st.st_size)
                saida.append((i, c, m, lido, status))
        except Falha as f:
            saida.append((i, None, None, 0, f.motivo))
        finally:
            os.close(fd)
    return saida


def tarefa_hash_amostra(lote):
    L = leitor_local()
    arqs = _G["arqs"]
    saida = []
    for i in lote:
        a = arqs[i]
        try:
            fd, st = L.abrir(a)
        except Falha as f:
            saida.append((i, None, False, 0, f.motivo))
            continue
        try:
            tam = a.tam
            h = hashlib.blake2b(digest_size=16)
            h.update(tam.to_bytes(8, "little"))
            if tam <= 3 * JANELA_HASH:
                b = L.ler(fd, 0, tam)
                h.update(b)
                lido = len(b)
                completo = True
            else:
                lido = 0
                for off in (0, (tam - JANELA_HASH) // 2, tam - JANELA_HASH):
                    b = L.ler(fd, off, JANELA_HASH)
                    h.update(b)
                    lido += len(b)
                completo = False
            saida.append((i, h.hexdigest(), completo, lido, None))
        except Falha as f:
            saida.append((i, None, False, 0, f.motivo))
        finally:
            os.close(fd)
    return saida


def tarefa_hash_completo(lote):
    L = leitor_local()
    arqs = _G["arqs"]
    saida = []
    for i in lote:
        a = arqs[i]
        try:
            fd, st = L.abrir(a)
        except Falha as f:
            saida.append((i, None, 0, f.motivo))
            continue
        try:
            h = hashlib.blake2b(digest_size=32)
            h.update(a.tam.to_bytes(8, "little"))
            off = 0
            lido = 0
            while True:
                b = L.ler(fd, off, MIB)
                if not b:
                    break
                h.update(b)
                off += len(b)
                lido += len(b)
            saida.append((i, h.hexdigest(), lido, None))
        except Falha as f:
            saida.append((i, None, 0, f.motivo))
        finally:
            os.close(fd)
    return saida


def comprimir(codec, dados):
    if codec.startswith("zstd-"):
        return zstd.compress(dados, level=int(codec[5:]))
    if codec == "xz-9e":
        filtros = [{"id": lzma.FILTER_LZMA2, "preset": 9 | lzma.PRESET_EXTREME, "dict_size": max(len(dados), 4096)}]
        return lzma.compress(dados, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64, filters=filtros)
    if codec == "bzip2-9":
        return bz2.compress(dados, 9)
    raise ValueError(codec)


def tarefa_amostra(item):
    L = leitor_local()
    i, blocos = item
    a = _G["arqs"][i]
    res = {"i": i, "linhas": [], "tempos": dict.fromkeys(CODECS, 0.0), "lido": 0, "rt_falhas": 0, "erro": None}
    try:
        fd, st = L.abrir(a)
    except Falha as f:
        res["erro"] = f.motivo
        return res
    com_transf = a.classe in CLASSES_TRANSFORM
    try:
        for b, lento in blocos:
            dados = L.ler(fd, b * TAMANHO_BLOCO, TAMANHO_BLOCO)
            n = len(dados)
            if n == 0:
                continue
            res["lido"] += n
            tam_c = {}
            fb = 0
            for k, codec in enumerate(CODECS):
                if codec in CODECS_LENTOS and not lento:
                    continue
                t0 = time.perf_counter()
                c = comprimir(codec, dados)
                res["tempos"][codec] += time.perf_counter() - t0
                if len(c) >= n:
                    fb |= 1 << k
                tam_c[codec] = min(len(c), n)
                if codec == "zstd-3" and zstd.decompress(c) != dados:
                    res["rt_falhas"] += 1
            escolha = -1
            melhor = 0
            tr = [0] * len(TRANSFORMS)
            if com_transf:
                melhor = tam_c["zstd-3"]
                escolha = 0
                for k, (nome, tipo, w) in enumerate(TRANSFORMS):
                    tr[k] = min(len(zstd.compress(aplicar_transform(dados, tipo, w), level=3)), n)
                    if tr[k] < melhor:
                        melhor = tr[k]
                        escolha = k + 1
            res["linhas"].append(
                [n, tam_c["zstd-1"], tam_c["zstd-3"], tam_c["zstd-9"], 1 if lento else 0, tam_c.get("zstd-19", 0), tam_c.get("xz-9e", 0), tam_c.get("bzip2-9", 0), fb, escolha, melhor] + tr
            )
    except Falha as f:
        res["erro"] = f.motivo
    finally:
        os.close(fd)
    return res


def tarefa_delta(par):
    L = leitor_local()
    arqs = _G["arqs"]
    i_ant, i_nov, off, n = par
    res = {"ant": i_ant, "nov": i_nov, "erro": None, "n": 0, "z3": 0, "patch": 0, "ok": True, "lido_ant": 0, "lido_nov": 0}
    try:
        fa, _ = L.abrir(arqs[i_ant])
    except Falha as f:
        res["erro"] = f.motivo
        return res
    try:
        fn, _ = L.abrir(arqs[i_nov])
    except Falha as f:
        os.close(fa)
        res["erro"] = f.motivo
        return res
    try:
        ref = L.ler(fa, off, n)
        novo = L.ler(fn, off, n)
        res["lido_ant"] = len(ref)
        res["lido_nov"] = len(novo)
        if not ref or not novo:
            res["erro"] = "janela_vazia"
            return res
        wl = min(27, max(21, math.ceil(math.log2(len(ref) + len(novo)))))
        d = zstd.ZstdDict(ref, is_raw=True)
        opc = {zstd.CompressionParameter.compression_level: 3, zstd.CompressionParameter.window_log: wl, zstd.CompressionParameter.enable_long_distance_matching: 1}
        c = zstd.compress(novo, options=opc, zstd_dict=d.as_prefix)
        res["ok"] = zstd.decompress(c, options={zstd.DecompressionParameter.window_log_max: wl}, zstd_dict=d.as_prefix) == novo
        res["n"] = len(novo)
        res["patch"] = len(c)
        res["z3"] = len(zstd.compress(novo, level=3))
    except Falha as f:
        res["erro"] = f.motivo
    finally:
        os.close(fa)
        os.close(fn)
    return res


class ErroUso(Exception):
    pass


OPC_PADRAO = {
    "raiz": None,
    "saida": None,
    "config": None,
    "amostra_mib": None,
    "amostra_mib_lento": None,
    "max_arquivos": 0,
    "hash_completo": False,
    "rapido": False,
    "revelar_nomes": False,
    "semente": 1,
    "processos": 4,
    "max_pares": None,
    "delta_mib": None,
    "prof_max": 64,
    "reamostras": None,
    "min_arquivos_confianca": 5,
    "tol_tamanho": 0.5,
    "max_unidades": 200000,
    "cruzar_dispositivos": False,
}


class Opc:
    def __init__(self, **kw):
        d = dict(OPC_PADRAO)
        for k, v in kw.items():
            if k not in d:
                raise ErroUso("opcao desconhecida: " + k)
            d[k] = v
        r = d["rapido"]
        for k, normal, rapido in (("amostra_mib", 256, 16), ("amostra_mib_lento", 32, 4), ("max_pares", 200, 20), ("delta_mib", 16, 4), ("reamostras", 1000, 200)):
            if d[k] is None:
                d[k] = rapido if r else normal
        d["processos"] = max(1, min(int(d["processos"]), MAX_PROCESSOS))
        d["classif_completa"] = not r
        self.__dict__.update(d)


class Estado:
    def __init__(self, opc, sal=None):
        self.opc = opc
        self.anon = Anon(opc.revelar_nomes, sal)
        self.v = None
        self.raiz_fd = None
        self.raiz_real = None
        self.tempos = {}
        self.base_pronta = False
        self.grupos_dup = []
        self.familias = []
        self.pares_medidos = []
        self.varredura = None
        self.amostra = None
        self.arrays = {}
        self.por_arquivo_amostra = {}
        self.boot = {}
        self.dedup_info = {}
        self.pares_erros = Counter()
        self.pares_total = 0
        self.delta_vetores = {}
        self.plano = {}

    def cronometrar(self, fase, t0):
        self.tempos[fase] = self.tempos.get(fase, 0.0) + (time.time() - t0)

    def abrir(self):
        if self.raiz_fd is not None:
            return
        if not self.opc.raiz or not os.path.isdir(self.opc.raiz):
            raise ErroUso("--raiz precisa ser um diretorio existente")
        self.raiz_fd, self.raiz_real = abrir_raiz(self.opc.raiz)
        if self.opc.saida:
            s = os.path.realpath(self.opc.saida)
            if s == self.raiz_real or s.startswith(self.raiz_real.rstrip(os.sep) + os.sep):
                raise ErroUso("--saida nao pode ficar dentro de --raiz (a ferramenta so le a raiz)")

    def fechar(self):
        if self.raiz_fd is not None:
            try:
                os.close(self.raiz_fd)
            except OSError:
                pass
            self.raiz_fd = None
        _G.clear()


def percentis_tamanho(tams):
    t = np.asarray(sorted(tams), dtype=np.float64)
    if t.size == 0:
        return {}
    cum = np.cumsum(t)
    total = cum[-1]
    saida = {
        "n": int(t.size),
        "media": float(t.mean()),
        "p50": float(np.percentile(t, 50)),
        "p90": float(np.percentile(t, 90)),
        "p99": float(np.percentile(t, 99)),
        "p99_9": float(np.percentile(t, 99.9)),
        "max": float(t[-1]),
    }
    if total > 0:
        for q in (50, 90, 99):
            saida["tamanho_abaixo_do_qual_ficam_%d_pct_dos_bytes" % q] = float(t[min(t.size - 1, int(np.searchsorted(cum, total * q / 100.0)))])
    return saida


def etapa_base(est):
    if est.base_pronta:
        return
    o = est.opc
    est.abrir()
    t0 = time.time()
    est.v = varrer_arvore(est.raiz_fd, o.max_arquivos, o.prof_max, not o.cruzar_dispositivos)
    est.v.raiz_real = est.raiz_real
    _G.update({"arqs": est.v.arquivos, "dirs": est.v.dirs, "raiz_fd": est.raiz_fd, "classif_completa": o.classif_completa})
    est.cronometrar("inventario", t0)
    t0 = time.time()
    v = est.v
    for lote_res in executar(tarefa_classificar, lotes(range(len(v.arquivos)), 128), o.processos):
        for i, c, m, lido, status in lote_res:
            a = v.arquivos[i]
            a.lido_classif = lido
            if c is None:
                a.classe = "ilegivel"
                a.status = status
            else:
                a.classe = c
                a.motivo_classe = m
                if status:
                    a.status = status
    est.cronometrar("classificacao", t0)
    est.base_pronta = True


def legivel(a):
    return a.classe is not None and a.classe != "ilegivel"


def etapa_dedup(est):
    o = est.opc
    v = est.v
    t0 = time.time()
    por_tam = defaultdict(list)
    for a in v.arquivos:
        if a.tam > 0 and legivel(a):
            por_tam[a.tam].append(a.i)
    cand = [ids for ids in por_tam.values() if len(ids) > 1]
    ids_cand = [i for ids in cand for i in ids]
    info = {
        "modo": "hash_completo_em_todos_os_candidatos_por_tamanho" if o.hash_completo else "pre_filtro_tamanho+hash_de_amostra, hash_completo_so_nos_candidatos",
        "arquivos_candidatos_por_tamanho": len(ids_cand),
        "bytes_candidatos_por_tamanho": int(sum(v.arquivos[i].tam for i in ids_cand)),
        "arquivos_candidatos_apos_hash_de_amostra": 0,
        "bytes_lidos_hash_amostra": 0,
        "bytes_lidos_hash_completo": 0,
    }
    grupos = []
    if o.hash_completo:
        pend = ids_cand
    else:
        res = {}
        for lote_res in executar(tarefa_hash_amostra, lotes(ids_cand, 64), o.processos):
            for i, h, completo, lido, status in lote_res:
                a = v.arquivos[i]
                a.lido_dedup += lido
                info["bytes_lidos_hash_amostra"] += lido
                if h is None:
                    a.status = a.status or status
                else:
                    a.h_amostra = h
                    res[i] = (h, completo)
        gr = defaultdict(list)
        for i in sorted(res):
            gr[(v.arquivos[i].tam, res[i][0])].append(i)
        pend = []
        for (tam, h), ids in gr.items():
            if len(ids) < 2:
                continue
            info["arquivos_candidatos_apos_hash_de_amostra"] += len(ids)
            if res[ids[0]][1]:
                grupos.append((h, ids, tam))
            else:
                pend.extend(ids)
    if pend:
        gr = defaultdict(list)
        for lote_res in executar(tarefa_hash_completo, lotes(pend, 16), o.processos):
            for i, h, lido, status in lote_res:
                a = v.arquivos[i]
                a.lido_dedup += lido
                info["bytes_lidos_hash_completo"] += lido
                if h is None:
                    a.status = a.status or status
                else:
                    a.h_completo = h
                    gr[(a.tam, h)].append(i)
        if o.hash_completo:
            info["arquivos_candidatos_apos_hash_de_amostra"] = len(ids_cand)
        for (tam, h), ids in gr.items():
            if len(ids) >= 2:
                grupos.append((h, sorted(ids), tam))
    grupos.sort(key=lambda g: g[1][0])
    for h, ids, tam in grupos:
        for i in ids[1:]:
            v.arquivos[i].redundante = True
    est.grupos_dup = grupos
    est.dedup_info = info
    est.cronometrar("dedup", t0)


def montar_dedup(est):
    v = est.v
    info = dict(est.dedup_info)
    total = sum(a.tam for a in v.arquivos)
    bytes_classe = Counter()
    for a in v.arquivos:
        bytes_classe[a.classe] += a.tam
    dup_classe = Counter()
    n_red = 0
    for h, ids, tam in est.grupos_dup:
        for i in ids[1:]:
            dup_classe[v.arquivos[i].classe] += tam
            n_red += 1
    dup = sum(dup_classe.values())
    top = sorted(est.grupos_dup, key=lambda g: -(len(g[1]) - 1) * g[2])[:10]
    lista = []
    for h, ids, tam in top:
        item = {
            "id": est.anon.id("D", h.encode()),
            "copias": len(ids),
            "tamanho": tam,
            "bytes_redundantes": (len(ids) - 1) * tam,
            "classe": v.arquivos[ids[0]].classe,
        }
        if est.anon.revelar:
            item["caminhos"] = [est.anon.nome(v.caminho(v.arquivos[i].dir, v.arquivos[i].nome)) for i in ids[:5]]
        lista.append(item)
    copias = [len(g[1]) for g in est.grupos_dup]
    info.update(
        {
            "grupos_de_duplicatas": len(est.grupos_dup),
            "arquivos_redundantes": n_red,
            "bytes_duplicados": dup,
            "pct_bytes_duplicados": dup / total if total else 0.0,
            "copias_por_grupo_media": float(np.mean(copias)) if copias else 0.0,
            "copias_por_grupo_max": max(copias) if copias else 0,
            "por_classe": {
                c: {"bytes_classe": bytes_classe[c], "bytes_duplicados": dup_classe[c], "pct": dup_classe[c] / bytes_classe[c] if bytes_classe[c] else 0.0}
                for c in CLASSES
                if bytes_classe[c] > 0
            },
            "maiores_grupos": lista,
        }
    )
    return info


RE_DATA = re.compile(r"\d{4}[-_.]?\d{2}[-_.]?\d{2}([t_ -]?\d{2}[:_.-]?\d{2}([:_.-]?\d{2})?)?")
RE_VERSAO = re.compile(r"(?<![a-z0-9])(v|ver|version|rev|r)?[-_. ]?\d+(\.\d+)+")
RE_COPIA = re.compile(r"[-_. ]*\(\d+\)|[-_. ]+(copy|copia|cópia|bak|backup|old|final|novo|new)\b")
RE_SUFIXO_NOME = re.compile(r"(?<![a-z0-9])(?:v|ver|rev|r)\d+(?![a-z0-9])")
RE_DIGITOS = re.compile(r"\d+")
RE_COLAPSO = re.compile(r"#(?:[-_. ]*#)+")


def normalizar(s):
    s = s.lower()
    s = RE_DATA.sub("#", s)
    s = RE_VERSAO.sub("#", s)
    s = RE_COPIA.sub("", s)
    s = RE_SUFIXO_NOME.sub("#", s)
    s = RE_DIGITOS.sub("#", s)
    return RE_COLAPSO.sub("#", s)


def chave_natural(rel):
    s = rel.decode("utf-8", "surrogateescape")
    return [int(t) if k % 2 else t for k, t in enumerate(re.split(r"(\d+)", s))]


def normalizar_nome(s):
    s = s.lower()
    s = RE_DATA.sub("#", s)
    s = RE_VERSAO.sub("#", s)
    s = RE_COPIA.sub("", s)
    s = RE_SUFIXO_NOME.sub("#", s)
    return RE_COLAPSO.sub("#", s)


def chave_dir(v, did, cache, normalizado):
    k = (did, normalizado)
    r = cache.get(k)
    if r is None:
        partes = []
        d = did
        while d > 0:
            pai, n = v.dirs[d]
            t = n.decode("utf-8", "surrogateescape")
            partes.append(normalizar(t) if normalizado else t)
            d = pai
        r = "/".join(reversed(partes))
        cache[k] = r
    return r


def nome_e_radical(a):
    nome = a.nome.decode("utf-8", "surrogateescape")
    return nome, (nome[: nome.rfind(".")] if a.ext != "" and "." in nome[1:] else nome)


def chave_familia(modo, v, a, cache):
    nome, stem = nome_e_radical(a)
    if modo == "A":
        return ("A", chave_dir(v, a.dir, cache, True), nome.lower())
    if modo == "B":
        return ("B", chave_dir(v, a.dir, cache, False), normalizar_nome(stem), a.ext)
    return ("C", chave_dir(v, a.dir, cache, True), normalizar_nome(stem), a.ext)


def montar_cadeias(v, grupos, tol):
    achadas = []
    for chave, membros in grupos.items():
        if len(membros) < 2:
            continue
        membros.sort(key=lambda a: (chave_natural(v.caminho(a.dir, a.nome)), a.mtime))
        cadeia = [membros[0]]
        for a in membros[1:]:
            ant = cadeia[-1]
            if max(a.tam, ant.tam) <= tol * min(a.tam, ant.tam):
                cadeia.append(a)
            else:
                if len(cadeia) >= 2:
                    achadas.append((chave, cadeia))
                cadeia = [a]
        if len(cadeia) >= 2:
            achadas.append((chave, cadeia))
    return achadas


def etapa_familias(est):
    o = est.opc
    v = est.v
    t0 = time.time()
    cache = {}
    tol = 1.0 + o.tol_tamanho
    livres = [a for a in v.arquivos if a.tam > 0 and legivel(a) and not a.redundante]
    familias = []
    for modo in ("A", "B", "C"):
        grupos = defaultdict(list)
        for a in livres:
            grupos[chave_familia(modo, v, a, cache)].append(a)
        achadas = montar_cadeias(v, grupos, tol)
        usados = set()
        for chave, cadeia in achadas:
            familias.append((chave, cadeia))
            usados.update(a.i for a in cadeia)
        livres = [a for a in livres if a.i not in usados]
    familias.sort(key=lambda f: f[1][0].i)
    for k, (chave, cadeia) in enumerate(familias):
        for j, a in enumerate(cadeia):
            a.familia = k
            a.base = j == 0
    est.familias = familias
    est.cronometrar("familias", t0)


def alocar_pares(n_por_classe, total, minimo=5):
    classes = [c for c in CLASSES if n_por_classe.get(c, 0) > 0]
    if not classes or total <= 0:
        return {}
    base = {c: min(n_por_classe[c], minimo) for c in classes}
    soma = sum(base.values())
    if soma > total:
        base = {c: max(1 if total >= len(classes) else 0, int(base[c] * total / soma)) for c in classes}
        return base
    restante = total - soma
    falta = {c: n_por_classe[c] - base[c] for c in classes}
    tot_falta = sum(falta.values())
    aloc = dict(base)
    if tot_falta > 0 and restante > 0:
        for c in classes:
            aloc[c] += min(falta[c], int(restante * falta[c] / tot_falta))
    return aloc


def etapa_delta(est):
    o = est.opc
    v = est.v
    t0 = time.time()
    pares = []
    for k, (chave, cadeia) in enumerate(est.familias):
        for ant, nov in zip(cadeia, cadeia[1:]):
            pares.append((ant.i, nov.i))
    por_classe = defaultdict(list)
    for p in pares:
        por_classe[v.arquivos[p[1]].classe].append(p)
    aloc = alocar_pares({c: len(x) for c, x in por_classe.items()}, o.max_pares)
    escolhidos = []
    n = o.delta_mib * MIB
    for ci, c in enumerate(CLASSES):
        lista = por_classe.get(c)
        if not lista or aloc.get(c, 0) <= 0:
            continue
        rng = np.random.default_rng([o.semente, 303, ci])
        idx = sorted(rng.choice(len(lista), size=min(aloc[c], len(lista)), replace=False))
        for j in idx:
            ia, inv = lista[j]
            m = min(v.arquivos[ia].tam, v.arquivos[inv].tam)
            off = 0
            if m > n:
                off = int(rng.integers(0, (m - n) // MIB + 1)) * MIB
            escolhidos.append((ia, inv, off, n))
    medidos = []
    erros = Counter()
    for r in executar(tarefa_delta, escolhidos, o.processos):
        v.arquivos[r["ant"]].lido_delta += r["lido_ant"]
        v.arquivos[r["nov"]].lido_delta += r["lido_nov"]
        if r["erro"]:
            erros[r["erro"]] += 1
            continue
        medidos.append(r)
    medidos.sort(key=lambda r: (r["nov"], r["ant"]))
    est.pares_medidos = medidos
    est.pares_erros = erros
    est.pares_total = len(pares)
    est.cronometrar("delta", t0)


def bootstrap_razao(num, den, rng, B):
    num = np.asarray(num, dtype=np.float64)
    den = np.asarray(den, dtype=np.float64)
    nf = len(num)
    out = np.full(B, np.nan)
    if nf == 0:
        return out
    passo = max(1, int(4e6 // nf))
    k = 0
    while k < B:
        m = min(passo, B - k)
        w = rng.poisson(1.0, size=(m, nf)).astype(np.float64)
        N = w @ num
        D = w @ den
        with np.errstate(divide="ignore", invalid="ignore"):
            out[k : k + m] = np.where(D > 0, N / D, np.nan)
        k += m
    return out


def intervalo(vet, nivel=0.95):
    x = vet[np.isfinite(vet)]
    if x.size == 0:
        return [None, None]
    a = (1.0 - nivel) / 2.0
    return [float(np.quantile(x, a)), float(np.quantile(x, 1.0 - a))]


def resumo_delta_classe(pares, rng, B, min_pares=5):
    n = np.array([p[0] for p in pares], dtype=np.float64)
    z3 = np.array([p[1] for p in pares], dtype=np.float64)
    pt = np.array([p[2] for p in pares], dtype=np.float64)
    if len(pares) == 0 or z3.sum() <= 0:
        return None, None
    r = float(pt.sum() / z3.sum())
    vet = bootstrap_razao(pt, z3, rng, B)
    return {
        "pares_medidos": len(pares),
        "bytes_janela_novos": int(n.sum()),
        "bytes_z3_sozinho": int(z3.sum()),
        "bytes_patch_from": int(pt.sum()),
        "razao_patch_sobre_z3": r,
        "ic95_razao_patch_sobre_z3": intervalo(vet),
        "ganho_delta_sobre_z3": 1.0 - r,
        "ic95_ganho_delta_sobre_z3": [None if x is None else 1.0 - x for x in intervalo(vet)[::-1]],
        "baixa_confianca": len(pares) < min_pares,
    }, vet


def montar_familias(est):
    o = est.opc
    v = est.v
    total = sum(a.tam for a in v.arquivos)
    em_fam = Counter()
    nao_base = Counter()
    bytes_classe = Counter()
    for a in v.arquivos:
        bytes_classe[a.classe] += a.tam
    n_membros = 0
    tam_fam = []
    for chave, cadeia in est.familias:
        tam_fam.append(len(cadeia))
        for j, a in enumerate(cadeia):
            n_membros += 1
            em_fam[a.classe] += a.tam
            if j > 0:
                nao_base[a.classe] += a.tam
    por_classe_pares = defaultdict(list)
    for r in est.pares_medidos:
        por_classe_pares[v.arquivos[r["nov"]].classe].append((r["n"], r["z3"], r["patch"]))
    estat = {}
    vetores = {}
    for ci, c in enumerate(CLASSES):
        if c in por_classe_pares:
            rng = np.random.default_rng([o.semente, 404, ci])
            d, vet = resumo_delta_classe(por_classe_pares[c], rng, o.reamostras)
            estat[c] = d
            vetores[c] = vet
            est.arrays["delta/" + c] = np.array(por_classe_pares[c], dtype=np.int64)
    todos = [p for lst in por_classe_pares.values() for p in lst]
    agregado = None
    if todos:
        agregado, vtot = resumo_delta_classe(todos, np.random.default_rng([o.semente, 405]), o.reamostras)
        vetores["_todos"] = vtot
        est.arrays["delta/_todos"] = np.array(todos, dtype=np.int64)
    top = sorted(est.familias, key=lambda f: -sum(a.tam for a in f[1]))[:10]
    medidos_por_nov = {r["nov"]: r for r in est.pares_medidos}
    lista = []
    for chave, cadeia in top:
        rs = [medidos_por_nov[a.i] for a in cadeia[1:] if a.i in medidos_por_nov]
        item = {
            "id": est.anon.id("F", ("\x00".join(chave)).encode("utf-8", "surrogateescape")),
            "membros": len(cadeia),
            "bytes": sum(a.tam for a in cadeia),
            "classe": cadeia[-1].classe,
            "razao_patch_sobre_z3_medida": (sum(r["patch"] for r in rs) / sum(r["z3"] for r in rs)) if rs and sum(r["z3"] for r in rs) > 0 else None,
        }
        if est.anon.revelar:
            item["exemplo"] = est.anon.nome(v.caminho(cadeia[0].dir, cadeia[0].nome))
        lista.append(item)
    est.delta_vetores = vetores
    return {
        "criterio": "tres passadas, cada arquivo entra na primeira que o agrupa: (A) diretorio-pai normalizado (digitos viram #) + mesmo nome; (B) mesmo diretorio + radical do nome sem datas, versoes e marcas de copia; (C) diretorio-pai normalizado + radical normalizado + mesma extensao; membros ordenados pelo caminho (ordem natural), com tamanhos consecutivos dentro de x%.1f" % (1.0 + o.tol_tamanho),
        "n_familias": len(est.familias),
        "n_membros": n_membros,
        "bytes_em_familias": int(sum(em_fam.values())),
        "pct_bytes_em_familias": sum(em_fam.values()) / total if total else 0.0,
        "bytes_nao_base": int(sum(nao_base.values())),
        "pct_bytes_nao_base": sum(nao_base.values()) / total if total else 0.0,
        "membros_por_familia": {"media": float(np.mean(tam_fam)) if tam_fam else 0.0, "max": max(tam_fam) if tam_fam else 0},
        "por_classe": {
            c: {
                "bytes_classe": bytes_classe[c],
                "bytes_em_familias": int(em_fam[c]),
                "pct_bytes_em_familias": em_fam[c] / bytes_classe[c] if bytes_classe[c] else 0.0,
                "bytes_nao_base": int(nao_base[c]),
                "pct_bytes_nao_base": nao_base[c] / bytes_classe[c] if bytes_classe[c] else 0.0,
            }
            for c in CLASSES
            if bytes_classe[c] > 0
        },
        "pares_possiveis": est.pares_total,
        "pares_medidos": len(est.pares_medidos),
        "pares_com_erro": dict(est.pares_erros),
        "pares_com_falha_de_roundtrip": sum(1 for r in est.pares_medidos if not r["ok"]),
        "janela_por_arquivo_mib": o.delta_mib,
        "delta_por_classe": estat,
        "delta_agregado_todas_as_classes": agregado,
        "maiores_familias": lista,
    }


def montar_inventario(est):
    v = est.v
    arqs = v.arquivos
    total = sum(a.tam for a in arqs)
    por_classe = defaultdict(lambda: [0, 0])
    por_ext = defaultdict(lambda: [0, 0, Counter()])
    for a in arqs:
        pc = por_classe[a.classe]
        pc[0] += 1
        pc[1] += a.tam
        pe = por_ext[a.ext]
        pe[0] += 1
        pe[1] += a.tam
        pe[2][a.classe] += a.tam
    exts = sorted(por_ext.items(), key=lambda kv: -kv[1][1])
    top_ext = [
        {"extensao": rotulo_extensao(e), "arquivos": n, "bytes": b, "pct_bytes": b / total if total else 0.0, "classe_dominante": cl.most_common(1)[0][0]}
        for e, (n, b, cl) in exts[:25]
    ]
    inc_class = sum(b for c, (n, b) in por_classe.items() if c in CLASSES_INCOMPRESSIVEIS)
    motivos = defaultdict(Counter)
    for a in arqs:
        if a.motivo_classe:
            m = a.motivo_classe.split(":")[0]
            motivos[a.classe][m] += 1
    ilegiveis = Counter(a.status for a in arqs if a.classe == "ilegivel")
    return {
        "n_arquivos": len(arqs),
        "n_diretorios": len(v.dirs),
        "bytes": total,
        "inventario_truncado_por_max_arquivos": v.truncada,
        "tamanho_dos_arquivos": percentis_tamanho([a.tam for a in arqs]),
        "por_classe": {c: {"arquivos": n, "bytes": b, "pct_bytes": b / total if total else 0.0} for c, (n, b) in sorted(por_classe.items(), key=lambda kv: -kv[1][1])},
        "por_extensao_top25_bytes": top_ext,
        "n_extensoes_distintas": len(por_ext),
        "matriz_classe_x_extensao_top": {
            rotulo_extensao(e): {c: b for c, b in cl.most_common(3)} for e, (n, b, cl) in exts[:10]
        },
        "fracao_incompressivel_por_classificacao": inc_class / total if total else 0.0,
        "bytes_incompressiveis_por_classificacao": inc_class,
        "classes_incompressiveis": list(CLASSES_INCOMPRESSIVEIS),
        "como_foram_classificados": {c: dict(m) for c, m in motivos.items()},
        "ilegiveis_por_motivo": dict(ilegiveis),
        "ignorados_por_motivo": dict(v.ignorados),
    }


def montar_registro(est):
    v = est.v
    arqs = v.arquivos
    lido = {
        "classificacao": sum(a.lido_classif for a in arqs),
        "dedup": sum(a.lido_dedup for a in arqs),
        "amostra_compressibilidade": sum(a.lido_amostra for a in arqs),
        "delta": sum(a.lido_delta for a in arqs),
    }
    abertos = sum(1 for a in arqs if a.lido_classif + a.lido_dedup + a.lido_amostra + a.lido_delta > 0)
    return {
        "arquivos_inventariados": len(arqs),
        "arquivos_com_leitura": abertos,
        "bytes_lidos_total": sum(lido.values()),
        "bytes_lidos_por_fase": lido,
        "bytes_inventariados": sum(a.tam for a in arqs),
        "fracao_lida_do_inventario": sum(lido.values()) / max(1, sum(a.tam for a in arqs)),
        "ilegiveis": sum(1 for a in arqs if a.classe == "ilegivel"),
        "ilegiveis_por_motivo": dict(Counter(a.status for a in arqs if a.classe == "ilegivel")),
        "ignorados_por_motivo": dict(v.ignorados),
        "diretorios_inacessiveis": v.dirs_inacessiveis,
        "nomes_com_bytes_invalidos_em_utf8": v.nomes_invalidos,
        "alterados_durante_leitura": sum(1 for a in arqs if a.status == "alterado_durante_leitura"),
        "abertura": "O_RDONLY|O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK, tentando O_NOATIME (negado em arquivos de outro dono: reabre sem)",
        "tempo_por_fase_s": {k: round(x, 3) for k, x in est.tempos.items()},
    }


def linhas_registro(est):
    v = est.v
    an = est.anon
    for a in v.arquivos:
        rel = v.caminho(a.dir, a.nome)
        item = {
            "id": an.id("A", rel),
            "ext": rotulo_extensao(a.ext),
            "classe": a.classe,
            "tamanho": a.tam,
            "bytes_lidos": {"classificacao": a.lido_classif, "dedup": a.lido_dedup, "amostra": a.lido_amostra, "delta": a.lido_delta},
            "status": a.status or "lido",
        }
        if an.revelar:
            item["caminho"] = an.nome(rel)
        yield item
    for motivo, rel, tam in v.ign_lista:
        item = {"id": an.id("A", rel), "ext": None, "classe": None, "tamanho": tam, "bytes_lidos": {}, "status": "ignorado:" + motivo}
        if an.revelar:
            item["caminho"] = an.nome(rel)
        yield item


def etapa_varrer(est):
    etapa_base(est)
    etapa_dedup(est)
    etapa_familias(est)
    etapa_delta(est)
    return montar_varredura(est)


def montar_varredura(est):
    o = est.opc
    inv = montar_inventario(est)
    dd = montar_dedup(est)
    fam = montar_familias(est)
    v = est.v
    por_classe = {}
    for c, d in inv["por_classe"].items():
        por_classe[c] = {
            "arquivos": d["arquivos"],
            "bytes": d["bytes"],
            "bytes_duplicados": dd["por_classe"].get(c, {}).get("bytes_duplicados", 0),
            "bytes_nao_base_em_familias": fam["por_classe"].get(c, {}).get("bytes_nao_base", 0),
            "bytes_em_familias": fam["por_classe"].get(c, {}).get("bytes_em_familias", 0),
        }
    est.varredura = {
        "meta": meta_execucao(est, "varrer"),
        "inventario": inv,
        "dedup": dd,
        "familias": fam,
        "por_classe_resumo": por_classe,
        "registro_leitura": montar_registro(est),
    }
    return est.varredura


def meta_execucao(est, etapa):
    o = est.opc
    return {
        "ferramenta": "perfil_dataset.py",
        "versao": VERSAO,
        "etapa": etapa,
        "data": time.strftime("%Y-%m-%d %H:%M:%S"),
        "semente": o.semente,
        "anonimizado": not o.revelar_nomes,
        "raiz": est.raiz_real if o.revelar_nomes else "(oculta na saida anonimizada)",
        "modo_rapido": o.rapido,
        "opcoes": {
            "amostra_mib": o.amostra_mib,
            "amostra_mib_lento": o.amostra_mib_lento,
            "max_arquivos": o.max_arquivos,
            "hash_completo": o.hash_completo,
            "processos": o.processos,
            "max_pares": o.max_pares,
            "delta_mib": o.delta_mib,
            "reamostras_bootstrap": o.reamostras,
            "min_arquivos_confianca": o.min_arquivos_confianca,
            "tol_tamanho_familias": o.tol_tamanho,
            "prof_max": o.prof_max,
            "mesmo_dispositivo": not o.cruzar_dispositivos,
        },
        "python": sys.version.split()[0],
        "zstd": zstd.zstd_version,
    }


def bootstrap_multi(pares, nf, rng, B):
    out = {k: np.full(B, np.nan) for k in pares}
    if nf == 0:
        return out
    passo = max(1, int(4e6 // nf))
    k = 0
    while k < B:
        m = min(passo, B - k)
        w = rng.poisson(1.0, size=(m, nf)).astype(np.float64)
        for nome, (num, den) in pares.items():
            N = w @ num
            D = w @ den
            with np.errstate(divide="ignore", invalid="ignore"):
                out[nome][k : k + m] = np.where(D > 0, N / D, np.nan)
        k += m
    return out


def planejar_amostra(est):
    o = est.opc
    v = est.v
    por_classe = defaultdict(list)
    for a in v.arquivos:
        if a.tam > 0 and legivel(a):
            por_classe[a.classe].append(a)
    plano = {}
    for ci, c in enumerate(CLASSES):
        arqs = por_classe.get(c)
        if not arqs:
            continue
        rng = np.random.default_rng([o.semente, 101, ci])
        bytes_c = sum(a.tam for a in arqs)
        unid = [(a.tam + TAMANHO_BLOCO - 1) // TAMANHO_BLOCO for a in arqs]
        N = sum(unid)
        pi = min(1.0, o.amostra_mib * MIB / bytes_c, o.max_unidades / N)
        pi_l = min(1.0, o.amostra_mib_lento * MIB / bytes_c, o.max_unidades / N)
        razao_l = min(1.0, pi_l / pi)
        itens = []
        total_lentos = 0
        for a, u in zip(arqs, unid):
            if pi >= 1.0:
                blocos = np.arange(u)
            else:
                esperado = pi * u
                k = int(esperado)
                if rng.random() < esperado - k:
                    k += 1
                if k >= u:
                    blocos = np.arange(u)
                elif k > 0:
                    blocos = np.sort(rng.choice(u, size=k, replace=False))
                else:
                    continue
            lentos = rng.random(len(blocos)) < razao_l
            total_lentos += int(lentos.sum())
            itens.append((a.i, [(int(b), bool(l)) for b, l in zip(blocos, lentos)]))
        if itens and total_lentos == 0:
            i0, bl = itens[0]
            bl[0] = (bl[0][0], True)
        plano[c] = {"pi": pi, "pi_lento": pi * razao_l, "itens": itens, "arquivos": len(arqs), "bytes": bytes_c, "censo": pi >= 1.0, "idx": {a.i: k for k, a in enumerate(arqs)}}
    return plano


def tarefas_da_amostra(plano, max_blocos=48):
    t = []
    for c in CLASSES:
        if c not in plano:
            continue
        for i, blocos in plano[c]["itens"]:
            for j in range(0, len(blocos), max_blocos):
                t.append((i, blocos[j : j + max_blocos]))
    return t


def etapa_amostrar(est):
    o = est.opc
    etapa_base(est)
    t0 = time.time()
    v = est.v
    plano = planejar_amostra(est)
    est.plano = plano
    linhas = defaultdict(list)
    tempos = defaultdict(lambda: dict.fromkeys(CODECS, 0.0))
    erros = Counter()
    rt_falhas = 0
    por_arq = {}
    for r in executar(tarefa_amostra, tarefas_da_amostra(plano), o.processos):
        a = v.arquivos[r["i"]]
        a.lido_amostra += r["lido"]
        rt_falhas += r["rt_falhas"]
        if r["erro"]:
            erros[r["erro"]] += 1
            a.status = a.status or r["erro"]
        k = plano[a.classe]["idx"][a.i]
        for row in r["linhas"]:
            linhas[a.classe].append([k] + row)
        for codec, t in r["tempos"].items():
            tempos[a.classe][codec] += t
        acc = por_arq.setdefault(a.i, [0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
        for row in r["linhas"]:
            acc[0] += row[0]
            acc[1] += row[1]
            acc[2] += row[2]
            acc[3] += row[3]
            acc[4] += row[0] * row[4]
            acc[5] += row[5]
            acc[6] += row[6]
            acc[7] += row[7]
            acc[8] += 1
            acc[9] += row[2] * row[4]
    est.por_arquivo_amostra = por_arq
    est.cronometrar("amostragem", t0)
    est.amostra = montar_amostra(est, plano, linhas, tempos, erros, rt_falhas)
    return est.amostra


def resumir_classe(est, c, ci, rows, plano_c, tempos_c):
    o = est.opc
    B = o.reamostras
    rows = rows[np.lexsort(rows.T[::-1])]
    k = rows[:, 0]
    n = rows[:, 1].astype(np.float64)
    lento = rows[:, 5].astype(np.float64)
    cols = {"zstd-1": rows[:, 2], "zstd-3": rows[:, 3], "zstd-9": rows[:, 4], "zstd-19": rows[:, 6], "xz-9e": rows[:, 7], "bzip2-9": rows[:, 8]}
    fb = rows[:, 9]
    escolha = rows[:, 10]
    melhor = rows[:, 11].astype(np.float64)
    tr = rows[:, 12:18]
    uniq = np.unique(k)
    por_arquivo = len(uniq) >= o.min_arquivos_confianca
    if por_arquivo:
        pos = np.searchsorted(uniq, k)
        m = len(uniq)

        def agg(x):
            return np.bincount(pos, weights=np.asarray(x, dtype=np.float64), minlength=m)

    else:
        m = len(k)

        def agg(x):
            return np.asarray(x, dtype=np.float64)

    incomp = (1.0 - cols["zstd-3"] / np.maximum(n, 1)) < LIMIAR_BLOCO_INCOMPRESSIVEL
    orig_f = agg(n)
    orig_l = agg(n * lento)
    pares = {}
    c3_lento = agg(cols["zstd-3"] * lento)
    for codec in CODECS_RAPIDOS:
        pares[codec] = (agg(cols[codec]), orig_f)
    for codec in CODECS_LENTOS:
        pares["rel:" + codec] = (agg(cols[codec]), c3_lento)
    pares["_incomp"] = (agg(n * incomp), orig_f)
    tem_transf = bool((escolha >= 0).any())
    if tem_transf:
        pares["_melhor_transform"] = (agg(melhor), orig_f)
    rng = np.random.default_rng([o.semente, 202, ci])
    vet = bootstrap_multi(pares, m, rng, B)
    r_z3 = float(pares["zstd-3"][0].sum() / pares["zstd-3"][1].sum())
    for codec in CODECS_LENTOS:
        vet[codec] = vet["rel:" + codec] * vet["zstd-3"]
    codecs = {}
    for idx, codec in enumerate(CODECS):
        if codec in CODECS_LENTOS:
            num, den = pares["rel:" + codec]
            r = float(num.sum() / den.sum() * r_z3) if den.sum() > 0 else None
        else:
            num, den = pares[codec]
            r = float(num.sum() / den.sum()) if den.sum() > 0 else None
        ic = intervalo(vet[codec])
        bits = (fb >> idx) & 1
        relev = lento > 0 if codec in CODECS_LENTOS else np.ones(len(k), dtype=bool)
        t = tempos_c[codec]
        orig_set = float((n * lento).sum()) if codec in CODECS_LENTOS else float(n.sum())
        codecs[codec] = {
            "razao": (1.0 / r) if r else None,
            "economia": (1.0 - r) if r is not None else None,
            "ic95_razao": [None if ic[1] is None or ic[1] <= 0 else 1.0 / ic[1], None if ic[0] is None or ic[0] <= 0 else 1.0 / ic[0]],
            "ic95_economia": [None if ic[1] is None else 1.0 - ic[1], None if ic[0] is None else 1.0 - ic[0]],
            "estimador": "razao do codec sobre zstd-3 nos blocos sorteados para o codec x razao zstd-3 de todos os blocos amostrados" if codec in CODECS_LENTOS else "razao de somas",
            "bytes_amostrados": int(orig_set),
            "blocos": int(relev.sum()),
            "blocos_armazenados_sem_compressao_pct": float(bits[relev].mean()) if relev.any() else 0.0,
            "vazao_comp_indicativa_MBps": (orig_set / 1e6 / t) if t > 0 else None,
        }
    inc = float(pares["_incomp"][0].sum() / orig_f.sum()) if orig_f.sum() > 0 else None
    res = {
        "arquivos_populacao": plano_c["arquivos"],
        "bytes_populacao": plano_c["bytes"],
        "arquivos_amostrados": int(len(uniq)),
        "unidades_amostradas": int(len(k)),
        "bytes_amostrados_rapidos": int(n.sum()),
        "bytes_amostrados_lentos": int((n * lento).sum()),
        "fracao_amostrada": float(n.sum() / plano_c["bytes"]),
        "censo": plano_c["censo"],
        "probabilidade_de_inclusao": plano_c["pi"],
        "baixa_confianca": plano_c["arquivos"] < o.min_arquivos_confianca,
        "ic95_por": "arquivo" if por_arquivo else "bloco",
        "codecs": codecs,
        "incompressivel_por_amostra": {
            "criterio": "bloco de 1 MiB com economia zstd-3 < %.0f%%" % (100 * LIMIAR_BLOCO_INCOMPRESSIVEL),
            "fracao_dos_bytes": inc,
            "ic95": intervalo(vet["_incomp"]),
        },
    }
    est.boot[(c, "_n")] = B
    for codec in CODECS:
        est.boot[(c, codec)] = vet[codec]
    est.boot[(c, "_incomp")] = vet["_incomp"]
    if tem_transf:
        sel = escolha >= 0
        num, den = pares["_melhor_transform"]
        r_m = float(num.sum() / den.sum())
        r_3 = float(pares["zstd-3"][0].sum() / pares["zstd-3"][1].sum())
        dif = vet["zstd-3"] - vet["_melhor_transform"]
        nomes = ["raw"] + [t[0] for t in TRANSFORMS]
        cont = Counter(int(x) for x in escolha[sel])
        res["transform"] = {
            "criterio": "melhor entre zstd-3 puro e {delta 1/2/4/8, byte_split 4/8}+zstd-3 por bloco de 1 MiB (oraculo: o planejador real nao conhece o resultado antes de comprimir; limite superior)",
            "razao_melhor_por_bloco": 1.0 / r_m,
            "economia_melhor_por_bloco": 1.0 - r_m,
            "ganho_pp_sobre_zstd3": 100.0 * (r_3 - r_m),
            "ic95_ganho_pp_sobre_zstd3": [None if x is None else 100.0 * x for x in intervalo(dif)],
            "blocos": int(sel.sum()),
            "escolha_pct_dos_blocos": {nomes[j]: cont.get(j, 0) / int(sel.sum()) for j in range(len(nomes))},
            "razao_por_transform_unico": {TRANSFORMS[j][0]: float(n[sel].sum() / np.maximum(1.0, tr[sel, j].sum())) for j in range(len(TRANSFORMS))},
        }
        est.boot[(c, "_melhor_transform")] = vet["_melhor_transform"]
    est.arrays["amostra/" + c] = rows
    est.arrays["amostra_meta/" + c] = np.array([plano_c["arquivos"], plano_c["bytes"]], dtype=np.int64)
    return res


def agregar_dataset(est, por_classe):
    B = est.opc.reamostras
    classes = [c for c in CLASSES if c in por_classe]
    total = sum(por_classe[c]["bytes_populacao"] for c in classes)
    if total <= 0:
        return None
    saida = {"bytes_considerados": total}
    for codec in CODECS + ["_incomp"]:
        pt = 0.0
        vet = np.zeros(B)
        ok = True
        for c in classes:
            b = por_classe[c]["bytes_populacao"]
            if codec == "_incomp":
                r = por_classe[c]["incompressivel_por_amostra"]["fracao_dos_bytes"]
            else:
                e = por_classe[c]["codecs"][codec]["economia"]
                r = None if e is None else 1.0 - e
            if r is None:
                ok = False
                break
            pt += b * r
            vet += b * est.boot[(c, codec)]
        if not ok:
            continue
        pt /= total
        vet /= total
        if codec == "_incomp":
            saida["incompressivel_por_amostra"] = {"fracao_dos_bytes": pt, "ic95": intervalo(vet)}
        else:
            ic = intervalo(vet)
            saida[codec] = {
                "razao": 1.0 / pt,
                "economia": 1.0 - pt,
                "ic95_razao": [None if ic[1] is None else 1.0 / ic[1], None if ic[0] is None else 1.0 / ic[0]],
                "ic95_economia": [None if ic[1] is None else 1.0 - ic[1], None if ic[0] is None else 1.0 - ic[0]],
            }
    return saida


def montar_amostra(est, plano, linhas, tempos, erros, rt_falhas):
    o = est.opc
    por_classe = {}
    for ci, c in enumerate(CLASSES):
        if c not in plano or not linhas.get(c):
            continue
        rows = np.array(linhas[c], dtype=np.int64)
        por_classe[c] = resumir_classe(est, c, ci, rows, plano[c], tempos[c])
    return {
        "meta": meta_execucao(est, "amostrar"),
        "metodo": {
            "unidade": "bloco independente de %d MiB, comprimido sozinho" % (TAMANHO_BLOCO // MIB),
            "estratificacao": "probabilidade de inclusao igual para todos os blocos da classe (pi = orcamento / bytes da classe), com arredondamento aleatorio por arquivo e escolha aleatoria dos blocos dentro de cada arquivo (determinista pela semente); censo quando a classe cabe no orcamento",
            "estimador": "razao de somas (bytes comprimidos / bytes originais) sobre os blocos sorteados: pondera por bytes; para os codecs lentos, que rodam num subconjunto aleatorio dos blocos, razao (codec / zstd-3) nesse subconjunto vezes a razao zstd-3 de todos os blocos amostrados (zstd-3 e a variavel auxiliar: reduz a variancia)",
            "ic95": "bootstrap de Poisson por arquivo (por bloco quando ha menos de %d arquivos amostrados), %d reamostras, mesmos pesos para todos os codecs da classe" % (o.min_arquivos_confianca, o.reamostras),
            "armazenamento_cru": "bloco que nao comprime e armazenado cru (min(comprimido, original)); contado em blocos_armazenados_sem_compressao_pct",
            "codecs_rapidos": {"codecs": CODECS_RAPIDOS, "orcamento_mib_por_classe": o.amostra_mib},
            "codecs_lentos": {"codecs": CODECS_LENTOS, "orcamento_mib_por_classe": o.amostra_mib_lento},
            "xz": "LZMA2 com dicionario do tamanho do bloco, CRC64 (como bench/syntrabench.py)",
            "bit_exatidao": "roundtrip zstd-3 conferido em todo bloco amostrado",
        },
        "roundtrip_zstd3_falhas": rt_falhas,
        "arquivos_com_erro_na_amostragem": dict(erros),
        "por_classe": por_classe,
        "dataset": agregar_dataset(est, por_classe),
        "vazao_nota": "vazao_comp_indicativa_MBps foi medida com ate %d processos concorrentes, sem fixar CPU, na maquina atual: so indicativa; a estimativa economica usa bench/resultados/medicoes.json (rotulada)" % o.processos,
    }


TIPOS_PARAM = ("medido", "verificado", "premissa", "inferencia_propria", "desconhecido", "nao_informado")
GB_H_POR_MBPS = 3.6
SEG_MES = 30 * 86400
PRECOS_JSON = os.path.join(BASE, "precos.json")
MEDICOES_JSON = os.path.join(BASE, "resultados", "medicoes.json")
CANDIDATOS_ASSINCRONOS = ["zstd-19", "xz-9e"]


def par(valor, unidade, fonte, tipo):
    return {"valor": valor, "unidade": unidade, "fonte": fonte, "tipo": tipo}


PARAMS_PADRAO = {
    "volume_mensal_bytes": par(5e15, "B/mes", "requisito do pedido: 5 PB/mes de processamento (PB = 10^15 B); bench/precos.json", "premissa"),
    "fracao_retida": par(0.90, "fracao do volume processado", "premissa: o cliente armazena 'um pouco menos' do que processa (docs/pesquisa-compressao-neural.md 9.1); trocar pelo numero real", "premissa"),
    "retencao_meses": par(60, "meses", "premissa do pedido anterior (T = 60 meses, bench/precos.json e docs/anexos/modelo_cliente.py)", "premissa"),
    "preco_armazenamento_gb_mes": par(0.023, "USD/GB-mes", "AWS S3 Standard, tabela publica (bench/precos.json); o preco real do cliente e desconhecido", "verificado"),
    "preco_nucleo_hora": par(0.0446, "USD/vCPU-h", "AWS c7i, Vantage 2026-11-03 (bench/precos.json); o custo de CPU real do cliente e desconhecido", "verificado"),
    "custo_movimento_usd_por_gb": par(0.0, "USD/GB movido", "DESCONHECIDO: usado 0 (mesma regiao) ate o cliente informar; ver sensibilidade (referencia: egress S3 US$0,09/GB)", "desconhecido"),
    "passagens_movimento": par(1, "passagens por mes", "DESCONHECIDO: quantas vezes o dado atravessa o transporte por mes", "desconhecido"),
    "leituras_por_gb_processado": par(3, "leituras (descompressoes) por GB processado", "premissa: modelo_cliente.py usa 1, 3 e 10; o volume lido real e desconhecido", "premissa"),
    "eficiencia_escala": par(1.0, "fator multiplicativo sobre a vazao por nucleo", "premissa do modelo_cliente.py (1,0, otimista: medido 0,42 a 0,95 em CPU hibrida)", "premissa"),
    "idade_recompressao_meses": par(1, "meses", "premissa: recompressao assincrona 1 mes depois da ingestao (dado ja em zstd-3)", "premissa"),
    "vazao_hash_mbps": par(1160, "MB/s por nucleo", "medido em outro corpus: blake2b em 1 P-core (docs/pesquisa-compressao-neural.md 10.8, 1,16 a 1,25 GB/s)", "medido"),
    "dedup_aplicavel": par(True, "booleano", "premissa: o produto elimina arquivos inteiros duplicados (gate F1: dedup de arquivo inteiro ganha das alternativas em arvores)", "premissa"),
    "delta_aplicavel": par(True, "booleano", "premissa: o produto usa referencia entre versoes (zstd --patch-from, janela longa) em familias de arquivos", "premissa"),
    "consumo_aceita_dado_comprimido": par(None, "booleano", "DESCONHECIDO: o que o cliente chama de 'processamento' e se o consumo aceita dado comprimido; sem isso o ganho de movimento nao e garantido", "desconhecido"),
    "vazao_mbps": par(None, "MB/s por nucleo por classe e codec", "opcional: {classe_do_perfil: {codec: {\"comp\": x, \"decomp\": y}}}; padrao = bench/resultados/medicoes.json (bloco de 1 MiB, outro corpus e outra CPU)", "medido"),
}
PARAMS_NUMERICOS_POSITIVOS = ("volume_mensal_bytes", "retencao_meses", "eficiencia_escala", "vazao_hash_mbps")
PARAMS_NAO_NEGATIVOS = ("preco_armazenamento_gb_mes", "preco_nucleo_hora", "custo_movimento_usd_por_gb", "passagens_movimento", "leituras_por_gb_processado", "idade_recompressao_meses")


def carregar_params(caminho=None):
    p = {k: dict(v) for k, v in PARAMS_PADRAO.items()}
    avisos = []
    if caminho:
        try:
            with open(caminho) as f:
                cfg = json.load(f)
        except (OSError, ValueError) as e:
            raise ErroUso("nao consegui ler o --config (%s)" % type(e).__name__)
        if not isinstance(cfg, dict):
            raise ErroUso("--config precisa ser um objeto JSON")
        for k, v in cfg.items():
            if k.startswith("_"):
                continue
            if k not in p:
                raise ErroUso("parametro desconhecido no --config: " + k)
            novo = dict(p[k])
            if isinstance(v, dict) and "valor" in v:
                novo["valor"] = v["valor"]
                for campo in ("unidade", "fonte", "tipo"):
                    if campo in v:
                        novo[campo] = v[campo]
                if "fonte" not in v:
                    novo["fonte"] = "(nao informada no --config)"
                    avisos.append("parametro %s sem fonte no --config" % k)
                if "tipo" not in v:
                    novo["tipo"] = "nao_informado"
                    avisos.append("parametro %s sem tipo no --config" % k)
            else:
                novo["valor"] = v
                novo["fonte"] = "(nao informada no --config)"
                novo["tipo"] = "nao_informado"
                avisos.append("parametro %s dado como valor puro, sem fonte nem tipo" % k)
            if novo["tipo"] not in TIPOS_PARAM:
                raise ErroUso("tipo invalido em %s: %r (use %s)" % (k, novo["tipo"], ", ".join(TIPOS_PARAM)))
            p[k] = novo
    for k in PARAMS_NUMERICOS_POSITIVOS:
        x = p[k]["valor"]
        if isinstance(x, bool) or not isinstance(x, (int, float)) or not x > 0:
            raise ErroUso("%s precisa ser numero > 0" % k)
    for k in PARAMS_NAO_NEGATIVOS:
        x = p[k]["valor"]
        if isinstance(x, bool) or not isinstance(x, (int, float)) or x < 0:
            raise ErroUso("%s precisa ser numero >= 0" % k)
    x = p["fracao_retida"]["valor"]
    if isinstance(x, bool) or not isinstance(x, (int, float)) or not 0 < x <= 1:
        raise ErroUso("fracao_retida precisa estar em (0, 1]")
    return p, avisos


def carregar_modulo(nome, arquivo):
    spec = importlib.util.spec_from_file_location(nome, arquivo)
    m = importlib.util.module_from_spec(spec)
    sys.modules[nome] = m
    spec.loader.exec_module(m)
    return m


def carregar_vazoes_medidas(caminho=None):
    caminho = caminho or MEDICOES_JSON
    if not os.path.exists(caminho):
        return {}
    eco = carregar_modulo("economia", os.path.join(BASE, "economia.py"))
    med = eco.carregar(caminho)
    saida = {}
    for r in eco.agregar(med):
        if r["modo"] == "bloco_1MiB":
            saida[(r["classe"], r["config"])] = (r["theta_c_MBps"], r["theta_d_MBps"], ",".join(r["arquivos"]))
    return saida


def vazao_classe(classe, codec, params, medidas):
    if codec == "stored":
        return None, None, "stored: copia, sem CPU de compressao"
    ov = params["vazao_mbps"]["valor"]
    if isinstance(ov, dict) and classe in ov and codec in ov[classe]:
        x = ov[classe][codec]
        return float(x["comp"]), float(x["decomp"]), "informado no --config (%s)" % params["vazao_mbps"]["tipo"]
    prox, sint = PROXY_MEDICOES[classe]
    t = medidas.get((prox, codec))
    if t is None:
        raise ErroUso("sem vazao para %s/%s: informe vazao_mbps no --config ou gere bench/resultados/medicoes.json" % (classe, codec))
    rot = "medido em outro corpus e outra CPU (medicoes.json, bloco de 1 MiB), classe '%s' usada como proxy de '%s'" % (prox, classe)
    if sint:
        rot += "; proxy SINTETICO"
    return t[0], t[1], rot


def cpu_gb(preco_h, mbps, ef):
    return preco_h / (mbps * ef * GB_H_POR_MBPS)


def linha_modelo_cliente(P_gb, ret, s_arm, s_mov, comp_mbs, dec_mbs, p, T, c_move, m, n_dec, cpu_h, ef=1.0):
    r_gb = ret * P_gb
    arm = s_arm * r_gb * p * T
    mov = s_mov * P_gb * c_move * m
    if comp_mbs is None:
        cpu = 0.0 * arm
    else:
        cpu = P_gb * cpu_gb(cpu_h, comp_mbs, ef) + P_gb * n_dec * cpu_gb(cpu_h, dec_mbs, ef)
    return arm, mov, cpu


def avaliar_classe(P, info, pol, vaz, s, rd, usar_dedup, usar_delta):
    V = P["volume_mensal_bytes"]["valor"]
    ret = P["fracao_retida"]["valor"]
    T = P["retencao_meses"]["valor"]
    p = P["preco_armazenamento_gb_mes"]["valor"]
    cpu_h = P["preco_nucleo_hora"]["valor"]
    cm = P["custo_movimento_usd_por_gb"]["valor"]
    mv = P["passagens_movimento"]["valor"]
    n_dec = P["leituras_por_gb_processado"]["valor"]
    ef = P["eficiencia_escala"]["valor"]
    idade = P["idade_recompressao_meses"]["valor"]
    v_hash = P["vazao_hash_mbps"]["valor"]
    P_gb = V / 1e9 * info["share"]
    r_gb = ret * P_gb
    d = info["dup_frac"] if usar_dedup else 0.0
    phi = info["fam_frac"] if (usar_delta and rd is not None) else 0.0
    sync = pol["sync"]
    if sync == "stored":
        s_c = 0.0
        fator_delta = 1.0
        comp_mbs = dec_mbs = None
    else:
        s_c = s["zstd-3"]
        fator_delta = 1.0 - phi * (1.0 - rd) if phi > 0 else 1.0
        comp_mbs, dec_mbs = vaz["zstd-3"][0], vaz["zstd-3"][1]
    stored_frac = (1.0 - d) * (1.0 - s_c) * fator_delta
    s_arm = 1.0 - stored_frac
    arm, mov, cpu = linha_modelo_cliente(P_gb, ret, s_arm, s_c, comp_mbs, dec_mbs, p, T, cm, mv, n_dec, cpu_h, ef)
    cpu_hash = P_gb * cpu_gb(cpu_h, v_hash, ef) if usar_dedup else 0.0
    cpu_delta = 0.0
    if sync != "stored" and phi > 0:
        cpu_delta = P_gb * (1.0 - d) * phi * cpu_gb(cpu_h, comp_mbs, ef)
    arm_as = mov_as = cpu_as = 0.0
    if pol.get("async") and sync != "stored":
        D = pol["async"]
        L = max(0.0, T - idade)
        esc = (1.0 - d) * fator_delta
        ds = s[D] - s_c
        cc_D = cpu_gb(cpu_h, vaz[D][0], ef)
        cd_D = cpu_gb(cpu_h, vaz[D][1], ef)
        cd_3 = cpu_gb(cpu_h, dec_mbs, ef)
        repack = (1.0 - d) * (cd_3 + cc_D)
        taxa = n_dec / (ret * T)
        arm_as = ds * esc * r_gb * L * p
        cpu_as = r_gb * repack + r_gb * taxa * (cd_D - cd_3) * L
    extra = cpu_hash + cpu_delta + cpu_as
    liquido = arm + arm_as + mov - cpu - extra
    return {
        "armazenamento_usd_mes": arm + arm_as,
        "movimento_usd_mes": mov,
        "cpu_usd_mes": cpu + extra,
        "cpu_compressao_e_leitura_usd_mes": cpu,
        "cpu_hash_dedup_usd_mes": cpu_hash,
        "cpu_delta_usd_mes": cpu_delta,
        "cpu_recompressao_assincrona_usd_mes": cpu_as,
        "armazenamento_sincrono_usd_mes": arm,
        "armazenamento_assincrono_usd_mes": arm_as,
        "liquido_usd_mes": liquido,
        "economia_armazenamento_fracao": s_arm,
    }


def avaliar_assincrono(P, info, vaz, s, rd, usar_dedup, usar_delta):
    ret = P["fracao_retida"]["valor"]
    T = P["retencao_meses"]["valor"]
    p = P["preco_armazenamento_gb_mes"]["valor"]
    cpu_h = P["preco_nucleo_hora"]["valor"]
    n_dec = P["leituras_por_gb_processado"]["valor"]
    ef = P["eficiencia_escala"]["valor"]
    idade = P["idade_recompressao_meses"]["valor"]
    V = P["volume_mensal_bytes"]["valor"]
    P_gb = V / 1e9 * info["share"]
    r_gb = ret * P_gb
    d = info["dup_frac"] if usar_dedup else 0.0
    phi = info["fam_frac"] if (usar_delta and rd is not None) else 0.0
    fator_delta = 1.0 - phi * (1.0 - rd) if phi > 0 else 1.0
    L = max(0.0, T - idade)
    s3 = s["zstd-3"]
    saida = []
    for D in CANDIDATOS_ASSINCRONOS:
        ds = s[D] - s3
        cc_D = cpu_gb(cpu_h, vaz[D][0], ef)
        cd_D = cpu_gb(cpu_h, vaz[D][1], ef)
        cd_3 = cpu_gb(cpu_h, vaz["zstd-3"][1], ef)
        repack = (1.0 - d) * (cd_3 + cc_D)
        taxa = n_dec / (ret * T)
        ganho_mes = ds * (1.0 - d) * fator_delta * p
        dcd = cd_D - cd_3
        denom = ganho_mes - taxa * dcd
        m_star = repack / denom if (denom > 0 and ds > 0) else None
        nv_gb = denom * L - repack
        taxa_mes_gb = r_gb * (1.0 - d) * 1e3 / SEG_MES
        saida.append(
            {
                "destino": D,
                "delta_economia_pp": 100.0 * ds,
                "custo_recompressao_usd_por_gb": repack,
                "ganho_armazenamento_usd_por_gb_mes": ganho_mes,
                "m_estrela_meses": m_star,
                "vida_restante_meses": L,
                "valor_liquido_por_gb_retido_usd": nv_gb,
                "nucleos_medios_recompressao": taxa_mes_gb * (1.0 / (vaz[D][0] * ef) + 1.0 / (vaz["zstd-3"][1] * ef)),
                "paga_na_vida_restante": bool(m_star is not None and m_star < L and nv_gb > 0),
            }
        )
    return saida


def escolher_politica(P, c, info, vaz, s, rd, usar_dedup, usar_delta):
    sem = avaliar_classe(P, info, {"sync": "stored"}, vaz, s, rd, usar_dedup, usar_delta)["liquido_usd_mes"]
    z3 = avaliar_classe(P, info, {"sync": "zstd-3"}, vaz, s, rd, usar_dedup, usar_delta)["liquido_usd_mes"]
    if not z3 > sem or s["zstd-3"] <= 0:
        return {"sync": "stored", "async": None, "avaliacao_assincrona": [], "texto": "stored (sem compressao; zstd-3 nao paga o CPU nesta classe)"}
    av = avaliar_assincrono(P, info, vaz, s, rd, usar_dedup, usar_delta)
    bons = [a for a in av if a["paga_na_vida_restante"]]
    if bons:
        melhor = max(bons, key=lambda a: a["valor_liquido_por_gb_retido_usd"])
        return {
            "sync": "zstd-3",
            "async": melhor["destino"],
            "avaliacao_assincrona": av,
            "texto": "zstd-3 sincrono + recompressao assincrona para %s (M*=%s meses)" % (melhor["destino"], f_num(melhor["m_estrela_meses"], 1)),
        }
    return {"sync": "zstd-3", "async": None, "avaliacao_assincrona": av, "texto": "zstd-3 sincrono (recompressao assincrona nao paga na vida restante)"}


def vetor_s(est, c, codec):
    v = est.boot.get((c, codec))
    return None if v is None else 1.0 - v


def monta_infos(var, amo, usar_classes=None):
    por = amo["por_classe"]
    total = sum(por[c]["bytes_populacao"] for c in por)
    infos = {}
    for c in CLASSES:
        if c not in por:
            continue
        rc = var["por_classe_resumo"].get(c, {"bytes": por[c]["bytes_populacao"], "bytes_duplicados": 0, "bytes_nao_base_em_familias": 0})
        unicos = max(1, rc["bytes"] - rc["bytes_duplicados"])
        infos[c] = {
            "bytes": por[c]["bytes_populacao"],
            "share": por[c]["bytes_populacao"] / total,
            "dup_frac": rc["bytes_duplicados"] / max(1, rc["bytes"]),
            "fam_frac": rc["bytes_nao_base_em_familias"] / unicos,
        }
    return infos


def estimar_economia(est, var, amo, params, avisos, medidas):
    B = est.opc.reamostras
    infos = monta_infos(var, amo)
    por = amo["por_classe"]
    delta = var["familias"]["delta_por_classe"]
    delta_ag = var["familias"]["delta_agregado_todas_as_classes"]
    vet_delta_total = est.delta_vetores.get("_todos")
    usar_dedup = bool(params["dedup_aplicavel"]["valor"])
    usar_delta = bool(params["delta_aplicavel"]["valor"])
    classes = [c for c in CLASSES if c in infos]
    vazoes = {}
    for c in classes:
        vazoes[c] = {cod: vazao_classe(c, cod, params, medidas) for cod in ["zstd-3", "zstd-19", "xz-9e"]}
    vz = {c: {cod: (t[0], t[1]) for cod, t in vazoes[c].items()} for c in classes}
    s_pt = {c: {cod: por[c]["codecs"][cod]["economia"] for cod in CODECS} for c in classes}
    s_vec = {c: {cod: vetor_s(est, c, cod) for cod in CODECS} for c in classes}
    rd_pt = {}
    rd_vec = {}
    rd_origem = {}
    for c in classes:
        if c in delta and delta[c] is not None:
            rd_pt[c] = delta[c]["razao_patch_sobre_z3"]
            rd_vec[c] = est.delta_vetores.get(c)
            rd_origem[c] = "medido nesta classe (%d pares)" % delta[c]["pares_medidos"]
        elif delta_ag is not None and infos[c]["fam_frac"] > 0:
            rd_pt[c] = delta_ag["razao_patch_sobre_z3"]
            rd_vec[c] = vet_delta_total
            rd_origem[c] = "premissa: razao agregada de todas as classes (sem pares medidos nesta classe)"
        else:
            rd_pt[c] = None
            rd_vec[c] = None
            rd_origem[c] = "nao medido (delta tratado como 0)"

    def rodada(usar_dd, usar_dl):
        saida = {}
        for c in classes:
            pol = escolher_politica(params, c, infos[c], vz[c], s_pt[c], rd_pt[c], usar_dd, usar_dl)
            pt = avaliar_classe(params, infos[c], pol, vz[c], s_pt[c], rd_pt[c], usar_dd, usar_dl)
            vecs = {k: [] for k in ("armazenamento_usd_mes", "movimento_usd_mes", "cpu_usd_mes", "liquido_usd_mes")}
            if all(s_vec[c][cod] is not None for cod in ("zstd-3", "zstd-19", "xz-9e")):
                sv = {cod: np.nan_to_num(s_vec[c][cod], nan=0.0) for cod in CODECS}
                rv = rd_vec[c]
                if rv is None:
                    rvv = None if rd_pt[c] is None else np.full(B, rd_pt[c])
                else:
                    rvv = np.nan_to_num(rv, nan=1.0)
                av = avaliar_classe(params, infos[c], pol, vz[c], sv, rvv, usar_dd, usar_dl)
                vecs = {k: np.asarray(av[k]) * np.ones(B) for k in vecs}
            saida[c] = {"pol": pol, "pt": pt, "vec": vecs}
        return saida

    r_sem = rodada(False, False)
    r_com = rodada(usar_dedup, usar_delta)

    def totais(r):
        t = {}
        for k in ("armazenamento_usd_mes", "movimento_usd_mes", "cpu_usd_mes", "liquido_usd_mes"):
            pt = sum(r[c]["pt"][k] for c in classes)
            vet = sum(r[c]["vec"][k] for c in classes) if classes and all(len(np.atleast_1d(r[c]["vec"][k])) == B for c in classes) else None
            t[k] = {"estimativa": pt, "ic95": intervalo(vet) if vet is not None else [None, None]}
        return t

    por_classe_out = {}
    for c in classes:
        a = r_com[c]
        info = infos[c]
        ic = {k: intervalo(np.asarray(a["vec"][k])) if len(np.atleast_1d(a["vec"][k])) == B else [None, None] for k in a["vec"]}
        por_classe_out[c] = {
            "bytes": info["bytes"],
            "pct_bytes": info["share"],
            "volume_mensal_tb": params["volume_mensal_bytes"]["valor"] * info["share"] / 1e12,
            "economia_compressao": {
                "zstd-3": s_pt[c]["zstd-3"],
                "ic95_zstd-3": por[c]["codecs"]["zstd-3"]["ic95_economia"],
                "zstd-19": s_pt[c]["zstd-19"],
                "xz-9e": s_pt[c]["xz-9e"],
            },
            "dedup_bytes_pct": info["dup_frac"],
            "familias_bytes_nao_base_pct_dos_unicos": info["fam_frac"],
            "delta_razao_patch_sobre_z3": rd_pt[c],
            "delta_origem": rd_origem[c],
            "economia_armazenamento_total_fracao": a["pt"]["economia_armazenamento_fracao"],
            "politica": a["pol"]["texto"],
            "politica_sincrona": a["pol"]["sync"],
            "politica_assincrona": a["pol"]["async"],
            "avaliacao_assincrona": a["pol"]["avaliacao_assincrona"],
            "armazenamento_usd_mes": a["pt"]["armazenamento_usd_mes"],
            "movimento_usd_mes": a["pt"]["movimento_usd_mes"],
            "cpu_usd_mes": a["pt"]["cpu_usd_mes"],
            "liquido_usd_mes": a["pt"]["liquido_usd_mes"],
            "ic95_liquido_usd_mes": ic["liquido_usd_mes"],
            "ic95_armazenamento_usd_mes": ic["armazenamento_usd_mes"],
            "detalhe_cpu": {k: a["pt"][k] for k in ("cpu_compressao_e_leitura_usd_mes", "cpu_hash_dedup_usd_mes", "cpu_delta_usd_mes", "cpu_recompressao_assincrona_usd_mes")},
            "compressao_apenas_liquido_usd_mes": r_sem[c]["pt"]["liquido_usd_mes"],
            "compressao_apenas_politica": r_sem[c]["pol"]["texto"],
            "baixa_confianca": por[c]["baixa_confianca"],
            "vazoes_usadas": {cod: {"comp_MBps": t[0], "decomp_MBps": t[1], "origem": t[2]} for cod, t in vazoes[c].items()},
            "nucleos_medios_sincrono_zstd3": params["volume_mensal_bytes"]["valor"] / SEG_MES / 1e6 * info["share"] / (vz[c]["zstd-3"][0] * params["eficiencia_escala"]["valor"]) if a["pol"]["sync"] != "stored" else 0.0,
        }
    total_pol = totais(r_com)
    total_sem = totais(r_sem)
    estimativa = {
        "parametros": params,
        "avisos_config": avisos,
        "formula": "armazenamento = s_arm x fracao_retida x volume_GB x retencao x preco_GB_mes; movimento = s_compressao x volume_GB x custo_movimento x passagens; CPU = volume_GB x (custo_comp_GB + leituras x custo_decomp_GB), custo_GB = preco_nucleo_h / (vazao_MBps x eficiencia x 3,6); identica a docs/anexos/modelo_cliente.py (conferida em autoteste). Acrescimos deste perfil: dedup e delta reduzem s_arm; recompressao assincrona segue a regra de M* de resultados/resumo.md secao 11",
        "por_classe": por_classe_out,
        "total": {
            "politica_recomendada_com_dedup_e_delta": total_pol,
            "compressao_apenas_politica_por_classe": total_sem,
            "dedup_aplicado": usar_dedup,
            "delta_aplicado": usar_delta,
        },
        "sensibilidade": sensibilidade(params, infos, vz, s_pt, rd_pt, classes, usar_dedup, usar_delta),
        "origem_dos_numeros": origem_dos_numeros(params, var, amo),
        "classes_sem_estimativa": [c for c, d in var["inventario"]["por_classe"].items() if c not in infos],
        "bytes_sem_estimativa": sum(d["bytes"] for c, d in var["inventario"]["por_classe"].items() if c not in infos),
    }
    return estimativa


def sensibilidade(params, infos, vz, s_pt, rd_pt, classes, usar_dedup, usar_delta):
    precos = {"s3_standard": (0.023, params["preco_nucleo_hora"]["valor"]), "b2": (0.00695, params["preco_nucleo_hora"]["valor"]), "glacier_ir": (0.004, params["preco_nucleo_hora"]["valor"]), "onprem": (0.002, 0.0135)}
    try:
        with open(PRECOS_JSON) as f:
            pj = json.load(f)
        precos = {
            "s3_standard": (pj["preco_gb_mes"]["s3_standard"]["valor"], pj["preco_nucleo_hora"]["nuvem"]["valor"]),
            "b2": (pj["preco_gb_mes"]["b2"]["valor"], pj["preco_nucleo_hora"]["nuvem"]["valor"]),
            "glacier_ir": (pj["preco_gb_mes"]["glacier_ir"]["valor"], pj["preco_nucleo_hora"]["nuvem"]["valor"]),
            "onprem": (pj["preco_gb_mes"]["onprem"]["valor"], pj["preco_nucleo_hora"]["onprem"]["valor"]),
        }
    except (OSError, KeyError, ValueError):
        pass

    def total(P):
        t = 0.0
        arm = mov = cpu = 0.0
        for c in classes:
            pol = escolher_politica(P, c, infos[c], vz[c], s_pt[c], rd_pt[c], usar_dedup, usar_delta)
            r = avaliar_classe(P, infos[c], pol, vz[c], s_pt[c], rd_pt[c], usar_dedup, usar_delta)
            t += r["liquido_usd_mes"]
            arm += r["armazenamento_usd_mes"]
            mov += r["movimento_usd_mes"]
            cpu += r["cpu_usd_mes"]
        return {"liquido": t, "armazenamento": arm, "movimento": mov, "cpu": cpu}

    def com(**kw):
        P = {k: dict(v) for k, v in params.items()}
        for k, x in kw.items():
            P[k]["valor"] = x
        return P

    ret_precos = []
    for ret in (0.80, 0.90, 0.95):
        for nome, (pg, ch) in precos.items():
            ret_precos.append({"fracao_retida": ret, "cenario_armazenamento": nome, "preco_gb_mes": pg, "preco_nucleo_hora": ch, **total(com(fracao_retida=ret, preco_armazenamento_gb_mes=pg, preco_nucleo_hora=ch))})
    mov = []
    for cm in (0.0, 0.01, 0.05, 0.09):
        for m in (1, 3):
            mov.append({"custo_movimento_usd_por_gb": cm, "passagens": m, **total(com(custo_movimento_usd_por_gb=cm, passagens_movimento=m))})
    leit = []
    for n in (1, 3, 10):
        leit.append({"leituras_por_gb": n, **total(com(leituras_por_gb_processado=n))})
    return {"fracao_retida_x_armazenamento": ret_precos, "movimento": mov, "leituras": leit}


def origem_dos_numeros(params, var, amo):
    return {
        "medido_no_dataset": [
            "numero de arquivos, bytes, percentis de tamanho, extensoes (inventario completo da raiz, so leitura de metadados)",
            "classe de cada arquivo (heuristica de extensao + bytes magicos + entropia de amostra de ate 48 KiB; nao e verdade absoluta)",
            "bytes de arquivos inteiros duplicados (hash de amostra e hash completo dos candidatos)",
            "familias de versoes (heuristica de nome/diretorio/tamanho) e a razao patch/zstd-3 em ate %d pares (janela de %d MiB por arquivo)" % (var["meta"]["opcoes"]["max_pares"], var["meta"]["opcoes"]["delta_mib"]),
            "razao de compressao por classe (zstd-1/3/9/19, xz-9e, bzip2-9) em blocos de 1 MiB sorteados, com IC95 por bootstrap",
            "fracao incompressivel por classificacao e por amostra (blocos com economia zstd-3 < 3%)",
        ],
        "medido_em_outro_corpus": [
            "vazao por nucleo de cada codec por classe: bench/resultados/medicoes.json (Silesia, Canterbury e sinteticos, i9-14900HX, tmpfs); a classe do cliente usa a classe medida mais parecida (proxy rotulado)",
            "vazao do hash (blake2b) do dedup: bench/resultados/dedup.md",
        ],
        "premissa": [
            "volume mensal de %s PB e fracao retida %s (o dataset e tratado como representativo do fluxo mensal)" % (f_num(params["volume_mensal_bytes"]["valor"] / 1e15, 2), f_num(params["fracao_retida"]["valor"], 2)),
            "precos (S3 Standard, c7i), retencao de %s meses, eficiencia de escala %s, leituras por GB %s" % (f_num(params["retencao_meses"]["valor"], 0), f_num(params["eficiencia_escala"]["valor"], 2), f_num(params["leituras_por_gb_processado"]["valor"], 0)),
            "dado duplicado e dado de familia tem a mesma compressibilidade da media da classe",
            "leituras uniformes no tempo; recompressao assincrona %s mes(es) apos a ingestao" % f_num(params["idade_recompressao_meses"]["valor"], 0),
            "o produto realiza dedup de arquivo inteiro e delta por familia (ver dedup_aplicavel e delta_aplicavel)",
            "o ganho de transform (delta/byte_split) e oraculo por bloco: limite superior, nao entra na estimativa economica",
        ],
        "desconhecido": [
            "o que o cliente chama de 'processamento' e se o consumo aceita dado comprimido (define se ha ganho de movimento e de CPU do cliente)",
            "custo real de movimento de dados e numero de passagens (usado US$ %s/GB e %s passagem(ns))" % (f_num(params["custo_movimento_usd_por_gb"]["valor"], 2), f_num(params["passagens_movimento"]["valor"], 0)),
            "volume realmente lido, padrao quente/frio e popularidade por objeto",
            "precos reais de armazenamento e computacao do cliente, descontos, custo de recuperacao e de saida",
            "se o dado ja chega comprimido ou criptografado na origem do fluxo (alem do que existe neste dataset)",
            "SLA de latencia de leitura, restricoes de LGPD e de residencia do dado",
            "se a amostra recebida representa o fluxo de 5 PB/mes (sazonalidade, novos tipos de dado)",
        ],
    }


def f_num(x, n=2):
    if x is None:
        return "-"
    if isinstance(x, str):
        return x
    if abs(x) >= 1000:
        return "{:,.0f}".format(x).replace(",", ".")
    return ("{:." + str(n) + "f}").format(x).replace(".", ",")


def f_pct(x, n=1):
    if x is None:
        return "-"
    return ("{:." + str(n) + "f}%").format(100.0 * x).replace(".", ",")


def f_bytes(b):
    if b is None:
        return "-"
    x = float(b)
    for u in ("B", "KiB", "MiB", "GiB", "TiB", "PiB"):
        if abs(x) < 1024 or u == "PiB":
            return (("{:.0f} {}" if u == "B" else "{:.2f} {}").format(x, u)).replace(".", ",")
        x /= 1024.0


def f_usd(x):
    if x is None:
        return "-"
    s = "US$ " + ("{:,.0f}".format(abs(x)) if abs(x) >= 100 else "{:,.2f}".format(abs(x)))
    s = s.replace(",", "X").replace(".", ",").replace("X", ".")
    return ("-" + s) if x < 0 else s


def f_ic(ic, fn):
    if not ic or ic[0] is None or ic[1] is None:
        return "[-]"
    return "[%s; %s]" % (fn(ic[0]), fn(ic[1]))


def tabela_md(cab, linhas):
    out = ["| " + " | ".join(cab) + " |", "|" + "|".join(["---"] * len(cab)) + "|"]
    for l in linhas:
        out.append("| " + " | ".join(str(c) for c in l) + " |")
    return "\n".join(out) + "\n"


def limites_do_perfil(var, amo, eco):
    L = [
        "A classificacao e uma heuristica (extensao, bytes magicos e entropia de uma amostra de ate 48 KiB por arquivo); arquivos mal classificados mudam a vazao e a politica atribuidas, nao a razao medida (que e calculada sobre os blocos reais da classe atribuida).",
        "A razao por classe vem de blocos de 1 MiB sorteados (ou de todos os blocos quando a classe cabe no orcamento); a amostra pode perder estruturas raras e nao ve redundancia entre blocos (cada bloco e comprimido sozinho, como no motor).",
        "O IC95 e de bootstrap por arquivo: cobre a variacao entre arquivos da amostra, nao a diferenca entre este dataset e o fluxo real de 5 PB/mes.",
        "Classes com menos de %d arquivos estao marcadas como baixa confianca: o IC por bloco subestima a incerteza entre arquivos." % var["meta"]["opcoes"]["min_arquivos_confianca"],
        "Dedup cobre so arquivos inteiros identicos; nao ha dedup por bloco nem por similaridade alem das familias; o hash de amostra so serve de pre-filtro, a confirmacao e pelo hash completo.",
        "Familias de versoes sao inferidas por nome, diretorio e tamanho; a razao patch/zstd-3 e medida em ate %d pares por janela de %d MiB (prefixo ou janela alinhada) e extrapolada a todos os pares da classe; pares fora da amostra podem ser mais ou menos parecidos." % (var["meta"]["opcoes"]["max_pares"], var["meta"]["opcoes"]["delta_mib"]),
        "O ganho de transform (delta/byte_split) e oraculo por bloco, um limite superior; a classe cientifico_numerico so aparece quando a entropia dos deltas cai pelo menos %.2f bit." % LIMIAR_GANHO_DELTA_BITS,
        "As vazoes por nucleo vem de bench/resultados/medicoes.json (outro corpus, CPU de notebook hibrido, tmpfs, sem I/O); classes sem medida propria usam proxy rotulado (log, ja comprimido e aleatorio usam classes SINTETICAS).",
        "O custo de CPU nao inclui I/O, rede, metadados, erasure coding, migracao do dado existente nem o custo de operar o indice de dedup a 5 PB/mes.",
        "Nao ha modelo de popularidade por objeto: as leituras sao uniformes no tempo.",
        "O volume mensal e a fracao retida sao premissas; o perfil do dataset recebido e extrapolado ao fluxo mensal sem correcao de sazonalidade.",
        "Desconhecido (perguntar ao cliente): o que ele chama de 'processamento' e se o consumo aceita dado comprimido; custo de movimento e passagens; volume lido e padrao quente/frio; precos reais; se o dado ja chega comprimido ou criptografado; SLA de latencia; restricoes de LGPD.",
        "Fracao incompressivel por classificacao conta so classes ja comprimidas, midia e criptografado; a por amostra conta qualquer bloco com economia zstd-3 menor que %.0f%% e e a mais confiavel." % (100 * LIMIAR_BLOCO_INCOMPRESSIVEL),
    ]
    if var["inventario"]["inventario_truncado_por_max_arquivos"]:
        L.append("O inventario foi TRUNCADO por --max-arquivos: todos os totais valem so para os arquivos inventariados.")
    ign = var["inventario"]["ignorados_por_motivo"]
    if ign:
        L.append("Entradas ignoradas (nao medidas): " + ", ".join("%s=%s" % (k, v) for k, v in sorted(ign.items())) + ".")
    il = var["inventario"]["ilegiveis_por_motivo"]
    if il:
        L.append("Arquivos ilegiveis (contados no inventario, sem classe nem estimativa): " + ", ".join("%s=%s" % (k, v) for k, v in sorted(il.items())) + ".")
    bc = [c for c, d in amo["por_classe"].items() if d["baixa_confianca"]]
    if bc:
        L.append("Classes de baixa confianca (poucos arquivos): " + ", ".join(bc) + ".")
    if eco is not None and eco.get("bytes_sem_estimativa"):
        L.append("Bytes sem estimativa economica (ilegiveis ou sem amostra): %s." % f_bytes(eco["bytes_sem_estimativa"]))
    if amo.get("roundtrip_zstd3_falhas"):
        L.append("ATENCAO: %d blocos falharam no roundtrip zstd-3." % amo["roundtrip_zstd3_falhas"])
    return L


def resumo_executivo(var, amo, eco):
    inv = var["inventario"]
    dd = var["dedup"]
    fam = var["familias"]
    ds = amo.get("dataset") or {}
    ts = inv["tamanho_dos_arquivos"]
    cls = list(inv["por_classe"].items())[:3]
    linhas = []
    linhas.append(
        "Dataset: %s arquivos, %s em %s diretorios (mediana %s, p99 %s); classes dominantes: %s."
        % (
            f_num(inv["n_arquivos"], 0),
            f_bytes(inv["bytes"]),
            f_num(inv["n_diretorios"], 0),
            f_bytes(ts.get("p50")),
            f_bytes(ts.get("p99")),
            ", ".join("%s %s" % (c, f_pct(d["pct_bytes"])) for c, d in cls) or "-",
        )
    )
    inc = ds.get("incompressivel_por_amostra", {})
    linhas.append(
        "Fracao incompressivel: %s por classificacao (midia, containers comprimidos, criptografado/aleatorio) e %s %s por amostra (blocos com economia zstd-3 < 3%%)."
        % (f_pct(inv["fracao_incompressivel_por_classificacao"]), f_pct(inc.get("fracao_dos_bytes")), f_ic(inc.get("ic95"), f_pct))
    )
    linhas.append(
        "Dedup de arquivo inteiro: %s dos bytes (%s) sao copias exatas, em %s grupos."
        % (f_pct(dd["pct_bytes_duplicados"]), f_bytes(dd["bytes_duplicados"]), f_num(dd["grupos_de_duplicatas"], 0))
    )
    da = fam.get("delta_agregado_todas_as_classes")
    linhas.append(
        "Familias de versoes: nenhuma encontrada; delta nao medido."
        if fam["n_familias"] == 0
        else "Familias de versoes: %s dos bytes em %s familias (%s sao versoes nao-base); o delta (patch-from) custa %s %s do zstd-3 isolado em %s pares medidos."
        % (
            f_pct(fam["pct_bytes_em_familias"]),
            f_num(fam["n_familias"], 0),
            f_pct(fam["pct_bytes_nao_base"]),
            f_pct(da["razao_patch_sobre_z3"]) if da else "-",
            f_ic(da["ic95_razao_patch_sobre_z3"], f_pct) if da else "[-]",
            f_num(fam["pares_medidos"], 0),
        )
    )
    z3 = ds.get("zstd-3")
    xz = ds.get("xz-9e")
    z19 = ds.get("zstd-19")
    linhas.append(
        "Compressao do dataset (blocos de 1 MiB, ponderada por bytes): zstd-3 economiza %s %s (razao %sx); zstd-19 %s; xz-9e %s %s."
        % (
            f_pct(z3["economia"]) if z3 else "-",
            f_ic(z3["ic95_economia"], f_pct) if z3 else "[-]",
            f_num(z3["razao"]) if z3 else "-",
            f_pct(z19["economia"]) if z19 else "-",
            f_pct(xz["economia"]) if xz else "-",
            f_ic(xz["ic95_economia"], f_pct) if xz else "[-]",
        )
    )
    maiores = sorted(amo["por_classe"].items(), key=lambda kv: -kv[1]["bytes_populacao"])[:4]
    linhas.append(
        "Ganho por classe (zstd-3, IC95): "
        + "; ".join("%s %s %s" % (c, f_pct(d["codecs"]["zstd-3"]["economia"]), f_ic(d["codecs"]["zstd-3"]["ic95_economia"], f_pct)) for c, d in maiores)
        + "."
    )
    if eco is None:
        linhas.append("Estimativa economica nao calculada nesta execucao (rode `estimar`).")
        linhas.append("Politica por classe: ver a estimativa economica.")
        linhas.append("Parametros: ver a estimativa economica.")
    else:
        t = eco["total"]
        sem = t["compressao_apenas_politica_por_classe"]["liquido_usd_mes"]
        com = t["politica_recomendada_com_dedup_e_delta"]["liquido_usd_mes"]
        P = eco["parametros"]
        arm = t["compressao_apenas_politica_por_classe"]["armazenamento_usd_mes"]["estimativa"]
        mov = t["compressao_apenas_politica_por_classe"]["movimento_usd_mes"]["estimativa"]
        cpu = t["compressao_apenas_politica_por_classe"]["cpu_usd_mes"]["estimativa"]
        linhas.append(
            "Economia mensal estimada so com compressao (politica por classe): %s %s = armazenamento %s + movimento %s - CPU %s."
            % (f_usd(sem["estimativa"]), f_ic(sem["ic95"], f_usd), f_usd(arm), f_usd(mov), f_usd(cpu))
        )
        asy = [c for c, d in eco["por_classe"].items() if d["politica_assincrona"]]
        linhas.append(
            "Com dedup e delta: %s %s por mes; recompressao assincrona recomendada em: %s; stored em: %s."
            % (
                f_usd(com["estimativa"]),
                f_ic(com["ic95"], f_usd),
                ", ".join("%s->%s" % (c, eco["por_classe"][c]["politica_assincrona"]) for c in asy) or "nenhuma classe",
                ", ".join(c for c, d in eco["por_classe"].items() if d["politica_sincrona"] == "stored") or "nenhuma classe",
            )
        )
        linhas.append(
            "Parametros (premissas): volume %s/mes, fracao retida %s, retencao %s meses, armazenamento US$ %s/GB-mes, nucleo US$ %s/h, %s leituras/GB, custo de movimento %s (DESCONHECIDO)."
            % (
                f_num(P["volume_mensal_bytes"]["valor"] / 1e15, 2) + " PB",
                f_num(P["fracao_retida"]["valor"]),
                f_num(P["retencao_meses"]["valor"], 0),
                f_num(P["preco_armazenamento_gb_mes"]["valor"], 4),
                f_num(P["preco_nucleo_hora"]["valor"], 4),
                f_num(P["leituras_por_gb_processado"]["valor"], 0),
                f_num(P["custo_movimento_usd_por_gb"]["valor"], 2),
            )
        )
    linhas.append(
        "Limites: amostra de blocos de 1 MiB e heuristicas de classe e familia; vazoes de outro corpus/CPU; desconhecido: o que o cliente chama de processamento e se o consumo aceita dado comprimido (ver secao de limites)."
    )
    return linhas


def montar_perfil(var, amo, eco, registro=None):
    return {
        "meta": var["meta"],
        "resumo_executivo": resumo_executivo(var, amo, eco),
        "inventario": var["inventario"],
        "dedup": var["dedup"],
        "familias": var["familias"],
        "compressibilidade": amo,
        "estimativa_economica": eco,
        "registro_leitura": registro or var["registro_leitura"],
        "limites_e_o_que_nao_sabemos": limites_do_perfil(var, amo, eco),
    }


def renderizar_md(perfil):
    var = perfil
    inv = perfil["inventario"]
    dd = perfil["dedup"]
    fam = perfil["familias"]
    amo = perfil["compressibilidade"]
    eco = perfil["estimativa_economica"]
    reg = perfil["registro_leitura"]
    anon = perfil["meta"]["anonimizado"]
    M = []
    M.append("# Perfil de dataset\n")
    M.append(
        "Gerado por `bench/perfil_dataset.py` (versao %s) em %s. Somente leitura; semente %s. %s\n"
        % (
            perfil["meta"]["versao"],
            perfil["meta"]["data"],
            perfil["meta"]["semente"],
            "Saida ANONIMIZADA: nenhum nome de arquivo ou diretorio; so extensoes, classes e identificadores hash salgados (sal aleatorio descartado)." if anon else "Saida COM NOMES (--revelar-nomes): tratar como confidencial.",
        )
    )
    M.append("## Resumo executivo\n")
    for k, l in enumerate(perfil["resumo_executivo"], 1):
        M.append("%d. %s" % (k, l))
    M.append("")
    M.append("## 1. Inventario\n")
    ts = inv["tamanho_dos_arquivos"]
    M.append(
        tabela_md(
            ["Item", "Valor"],
            [
                ["Arquivos", f_num(inv["n_arquivos"], 0)],
                ["Diretorios", f_num(inv["n_diretorios"], 0)],
                ["Bytes", "%s (%s B)" % (f_bytes(inv["bytes"]), f_num(inv["bytes"], 0))],
                ["Inventario truncado", "sim" if inv["inventario_truncado_por_max_arquivos"] else "nao"],
                ["Tamanho: media / mediana / p90 / p99 / p99,9 / max", " / ".join(f_bytes(ts.get(k)) for k in ("media", "p50", "p90", "p99", "p99_9", "max"))],
                ["Tamanho abaixo do qual ficam 50% / 90% / 99% dos bytes", " / ".join(f_bytes(ts.get("tamanho_abaixo_do_qual_ficam_%d_pct_dos_bytes" % q)) for q in (50, 90, 99))],
                ["Extensoes distintas", inv["n_extensoes_distintas"]],
            ],
        )
    )
    M.append("\nExtensoes por bytes (top 25)\n")
    M.append(tabela_md(["Extensao", "Arquivos", "Bytes", "% dos bytes", "Classe dominante"], [[e["extensao"], f_num(e["arquivos"], 0), f_bytes(e["bytes"]), f_pct(e["pct_bytes"]), e["classe_dominante"]] for e in inv["por_extensao_top25_bytes"]]))
    if inv["ignorados_por_motivo"]:
        M.append("\nEntradas ignoradas e por que\n")
        M.append(tabela_md(["Motivo", "Quantidade"], [[k, f_num(v, 0)] for k, v in sorted(inv["ignorados_por_motivo"].items())]))
    M.append("\n## 2. Classificacao (matriz classe x bytes)\n")
    M.append(
        tabela_md(
            ["Classe", "Arquivos", "Bytes", "% dos bytes", "Incompressivel por classificacao", "Como foi classificado (arquivos)"],
            [
                [
                    c,
                    f_num(d["arquivos"], 0),
                    f_bytes(d["bytes"]),
                    f_pct(d["pct_bytes"]),
                    "sim" if c in inv["classes_incompressiveis"] else "nao",
                    ", ".join("%s=%s" % (k, v) for k, v in sorted(inv["como_foram_classificados"].get(c, {}).items())) or "-",
                ]
                for c, d in inv["por_classe"].items()
            ],
        )
    )
    ds = amo.get("dataset") or {}
    inc = ds.get("incompressivel_por_amostra", {})
    M.append(
        "\nFracao incompressivel: **%s** por classificacao; **%s** %s por amostra (blocos de 1 MiB com economia zstd-3 menor que 3%%, ponderada por bytes).\n"
        % (f_pct(inv["fracao_incompressivel_por_classificacao"]), f_pct(inc.get("fracao_dos_bytes")), f_ic(inc.get("ic95"), f_pct))
    )
    M.append("\n## 3. Dedup de arquivo inteiro\n")
    M.append("Modo: %s. Candidatos por tamanho: %s arquivos (%s); apos hash de amostra: %s; bytes lidos no hash de amostra: %s; no hash completo: %s.\n" % (dd["modo"], f_num(dd["arquivos_candidatos_por_tamanho"], 0), f_bytes(dd["bytes_candidatos_por_tamanho"]), f_num(dd["arquivos_candidatos_apos_hash_de_amostra"], 0), f_bytes(dd["bytes_lidos_hash_amostra"]), f_bytes(dd["bytes_lidos_hash_completo"])))
    M.append("Grupos de duplicatas: %s; arquivos redundantes: %s; bytes duplicados: %s (%s do dataset).\n" % (f_num(dd["grupos_de_duplicatas"], 0), f_num(dd["arquivos_redundantes"], 0), f_bytes(dd["bytes_duplicados"]), f_pct(dd["pct_bytes_duplicados"])))
    M.append(tabela_md(["Classe", "Bytes da classe", "Bytes duplicados", "% da classe"], [[c, f_bytes(d["bytes_classe"]), f_bytes(d["bytes_duplicados"]), f_pct(d["pct"])] for c, d in dd["por_classe"].items()]))
    if dd["maiores_grupos"]:
        M.append("\nMaiores grupos de duplicatas (identificador hash salgado)\n")
        M.append(tabela_md(["Id", "Copias", "Tamanho", "Bytes redundantes", "Classe"] + (["Caminhos"] if not anon else []), [[g["id"], g["copias"], f_bytes(g["tamanho"]), f_bytes(g["bytes_redundantes"]), g["classe"]] + ([", ".join(g.get("caminhos", []))] if not anon else []) for g in dd["maiores_grupos"]]))
    M.append("\n## 4. Familias de versoes e delta\n")
    M.append("Criterio: %s.\n" % fam["criterio"])
    M.append(
        "Familias: %s com %s membros; bytes em familias: %s (%s); versoes nao-base: %s (%s). Pares possiveis: %s; medidos: %s (janela de %s MiB por arquivo); pares com falha de roundtrip: %s.\n"
        % (f_num(fam["n_familias"], 0), f_num(fam["n_membros"], 0), f_bytes(fam["bytes_em_familias"]), f_pct(fam["pct_bytes_em_familias"]), f_bytes(fam["bytes_nao_base"]), f_pct(fam["pct_bytes_nao_base"]), f_num(fam["pares_possiveis"], 0), f_num(fam["pares_medidos"], 0), fam["janela_por_arquivo_mib"], fam["pares_com_falha_de_roundtrip"])
    )
    linhas = []
    for c, d in fam["por_classe"].items():
        dl = fam["delta_por_classe"].get(c)
        linhas.append(
            [
                c,
                f_pct(d["pct_bytes_em_familias"]),
                f_pct(d["pct_bytes_nao_base"]),
                dl["pares_medidos"] if dl else 0,
                f_pct(dl["razao_patch_sobre_z3"]) if dl else "-",
                f_ic(dl["ic95_razao_patch_sobre_z3"], f_pct) if dl else "-",
                "sim" if dl and dl["baixa_confianca"] else ("nao" if dl else "-"),
            ]
        )
    M.append(tabela_md(["Classe", "% bytes em familias", "% bytes nao-base", "Pares medidos", "patch-from / zstd-3 isolado", "IC95", "Baixa confianca"], linhas))
    if fam["maiores_familias"]:
        M.append("\nMaiores familias\n")
        M.append(tabela_md(["Id", "Membros", "Bytes", "Classe", "patch/z3 medido"] + (["Exemplo"] if not anon else []), [[g["id"], g["membros"], f_bytes(g["bytes"]), g["classe"], f_pct(g["razao_patch_sobre_z3_medida"])] + ([g.get("exemplo", "")] if not anon else []) for g in fam["maiores_familias"]]))
    M.append("\n## 5. Compressibilidade por classe (blocos independentes de 1 MiB)\n")
    M.append(
        "Estimador: razao de somas ponderada por bytes; IC95 por bootstrap (%s). Cada celula: economia (razao x). Codecs rapidos em ate %s MiB por classe; lentos em ate %s MiB. 'cru%%' = blocos armazenados sem compressao.\n"
        % (amo["metodo"]["ic95"], amo["metodo"]["codecs_rapidos"]["orcamento_mib_por_classe"], amo["metodo"]["codecs_lentos"]["orcamento_mib_por_classe"])
    )
    linhas = []
    for c, d in amo["por_classe"].items():
        cd = d["codecs"]

        def cel(k, ic=False):
            x = cd[k]
            t = "%s (%sx)" % (f_pct(x["economia"]), f_num(x["razao"]))
            if ic:
                t += " " + f_ic(x["ic95_economia"], f_pct)
            return t

        linhas.append([c, f_num(d["arquivos_populacao"], 0), f_pct(d["fracao_amostrada"]), cel("zstd-1"), cel("zstd-3", True), cel("zstd-9"), cel("zstd-19"), cel("xz-9e", True), cel("bzip2-9"), f_pct(cd["zstd-3"]["blocos_armazenados_sem_compressao_pct"]), "BAIXA" if d["baixa_confianca"] else "ok"])
    M.append(tabela_md(["Classe", "Arquivos", "% amostrado", "zstd-1", "zstd-3 [IC95]", "zstd-9", "zstd-19", "xz-9e [IC95]", "bzip2-9", "cru%", "Confianca"], linhas))
    if ds:
        M.append("\nDataset inteiro (ponderado por bytes das classes amostradas): " + "; ".join("%s %s %s (%sx)" % (k, f_pct(ds[k]["economia"]), f_ic(ds[k]["ic95_economia"], f_pct), f_num(ds[k]["razao"])) for k in CODECS if k in ds) + ".\n")
    for c, d in amo["por_classe"].items():
        if "transform" in d:
            t = d["transform"]
            M.append("\nTransform por bloco em `%s` (%s): melhor por bloco economiza %s (razao %sx), ganho de %s pp sobre zstd-3 %s; escolha por bloco: %s.\n" % (c, t["criterio"], f_pct(t["economia_melhor_por_bloco"]), f_num(t["razao_melhor_por_bloco"]), f_num(t["ganho_pp_sobre_zstd3"]), f_ic(t["ic95_ganho_pp_sobre_zstd3"], f_num), ", ".join("%s=%s" % (k, f_pct(v, 0)) for k, v in t["escolha_pct_dos_blocos"].items() if v > 0)))
    M.append("\nVazao de compressao medida nesta execucao (indicativa, %s): " % amo["vazao_nota"] + "; ".join("%s %s" % (c, "/".join("%s=%s" % (k, f_num(v["vazao_comp_indicativa_MBps"], 0)) for k, v in d["codecs"].items() if v["vazao_comp_indicativa_MBps"])) for c, d in list(amo["por_classe"].items())[:3]) + " MB/s (primeiras classes).\n")
    M.append("\n## 6. Estimativa economica\n")
    if eco is None:
        M.append("Nao calculada.\n")
    else:
        M.append("Formula: " + eco["formula"] + ".\n")
        M.append("### Parametros (valor, fonte e tipo)\n")
        linhas = []
        for k, p in eco["parametros"].items():
            v = p["valor"]
            if isinstance(v, dict):
                v = "(por classe, ver config)"
            if v is None:
                v = "(sem valor)"
            elif isinstance(v, bool):
                v = "sim" if v else "nao"
            elif isinstance(v, (int, float)):
                v = f_num(v, 4)
            linhas.append([k, v, p["unidade"], p["tipo"], p["fonte"]])
        M.append(tabela_md(["Parametro", "Valor", "Unidade", "Tipo", "Fonte"], linhas))
        if eco["avisos_config"]:
            M.append("\nAvisos do --config: " + "; ".join(eco["avisos_config"]) + ".\n")
        M.append("\n### Por classe (politica recomendada, com dedup e delta quando aplicaveis)\n")
        linhas = []
        for c, d in eco["por_classe"].items():
            linhas.append([c, f_pct(d["pct_bytes"]), f_pct(d["economia_compressao"]["zstd-3"]), f_pct(d["dedup_bytes_pct"]), f_pct(d["economia_armazenamento_total_fracao"]), f_usd(d["armazenamento_usd_mes"]), f_usd(d["movimento_usd_mes"]), f_usd(d["cpu_usd_mes"]), f_usd(d["liquido_usd_mes"]), f_ic(d["ic95_liquido_usd_mes"], f_usd), d["politica"]])
        t = eco["total"]["politica_recomendada_com_dedup_e_delta"]
        linhas.append(["**TOTAL**", "100%", "", "", "", f_usd(t["armazenamento_usd_mes"]["estimativa"]), f_usd(t["movimento_usd_mes"]["estimativa"]), f_usd(t["cpu_usd_mes"]["estimativa"]), f_usd(t["liquido_usd_mes"]["estimativa"]), f_ic(t["liquido_usd_mes"]["ic95"], f_usd), ""])
        M.append(tabela_md(["Classe", "% bytes", "Economia zstd-3", "Dedup", "Economia de armazenamento total", "Armazenamento/mes", "Movimento/mes", "CPU/mes", "Liquido/mes", "IC95 liquido", "Politica"], linhas))
        M.append("\nValores em US$ por mes em regime estavel. Armazenamento = economia acumulada nos meses de retencao; liquido = armazenamento + movimento - CPU (compressao, leituras, hash do dedup, delta e recompressao).\n")
        ts_ = eco["total"]["compressao_apenas_politica_por_classe"]
        M.append("Apenas compressao (sem dedup nem delta): liquido %s %s.\n" % (f_usd(ts_["liquido_usd_mes"]["estimativa"]), f_ic(ts_["liquido_usd_mes"]["ic95"], f_usd)))
        asy = [(c, d) for c, d in eco["por_classe"].items() if d["avaliacao_assincrona"]]
        if asy:
            M.append("\n### Recompressao assincrona (regra de M* de resultados/resumo.md, secao 11)\n")
            linhas = []
            for c, d in asy:
                for a in d["avaliacao_assincrona"]:
                    linhas.append([c, a["destino"], f_num(a["delta_economia_pp"]), f_num(a["custo_recompressao_usd_por_gb"], 4), "nunca" if a["m_estrela_meses"] is None else f_num(a["m_estrela_meses"], 1), f_num(a["vida_restante_meses"], 0), f_num(a["nucleos_medios_recompressao"], 1), "sim" if a["paga_na_vida_restante"] else "nao"])
            M.append(tabela_md(["Classe", "Destino", "Delta economia (pp)", "Custo de recompressao (US$/GB)", "M* (meses)", "Vida restante (meses)", "Nucleos medios", "Paga na vida restante"], linhas))
        M.append("\n### Sensibilidade (liquido total em US$/mes)\n")
        S = eco["sensibilidade"]
        M.append(tabela_md(["Fracao retida", "Cenario de armazenamento", "US$/GB-mes", "Liquido/mes"], [[f_num(x["fracao_retida"]), x["cenario_armazenamento"], f_num(x["preco_gb_mes"], 5), f_usd(x["liquido"])] for x in S["fracao_retida_x_armazenamento"]]))
        M.append("\n" + tabela_md(["Custo de movimento (US$/GB)", "Passagens", "Movimento/mes", "Liquido/mes"], [[f_num(x["custo_movimento_usd_por_gb"]), x["passagens"], f_usd(x["movimento"]), f_usd(x["liquido"])] for x in S["movimento"]]))
        M.append("\n" + tabela_md(["Leituras por GB processado", "CPU/mes", "Liquido/mes"], [[x["leituras_por_gb"], f_usd(x["cpu"]), f_usd(x["liquido"])] for x in S["leituras"]]))
        M.append("\n## 7. O que e medido no dataset, o que e premissa e o que e desconhecido\n")
        for k, tit in (("medido_no_dataset", "Medido no dataset"), ("medido_em_outro_corpus", "Medido em outro corpus"), ("premissa", "Premissa"), ("desconhecido", "Desconhecido")):
            M.append("**%s**\n" % tit)
            for x in eco["origem_dos_numeros"][k]:
                M.append("- " + x)
            M.append("")
        M.append("Vazoes usadas por classe (proxy)\n")
        linhas = []
        for c, d in eco["por_classe"].items():
            v = d["vazoes_usadas"]["zstd-3"]
            linhas.append([c, f_num(v["comp_MBps"], 0), f_num(v["decomp_MBps"], 0), v["origem"]])
        M.append(tabela_md(["Classe", "zstd-3 comp MB/s", "zstd-3 decomp MB/s", "Origem"], linhas))
    M.append("\n## 8. Registro de leitura\n")
    M.append(
        tabela_md(
            ["Item", "Valor"],
            [
                ["Arquivos inventariados", f_num(reg["arquivos_inventariados"], 0)],
                ["Arquivos com leitura", f_num(reg["arquivos_com_leitura"], 0)],
                ["Bytes lidos (total)", "%s (%s do inventario)" % (f_bytes(reg["bytes_lidos_total"]), f_pct(reg["fracao_lida_do_inventario"], 2))],
                ["Bytes lidos por fase", ", ".join("%s=%s" % (k, f_bytes(v)) for k, v in reg["bytes_lidos_por_fase"].items())],
                ["Ilegiveis", "%s %s" % (reg["ilegiveis"], reg["ilegiveis_por_motivo"] or "")],
                ["Ignorados por motivo", str(reg["ignorados_por_motivo"] or "nenhum")],
                ["Diretorios inacessiveis", reg["diretorios_inacessiveis"]],
                ["Nomes com bytes invalidos em UTF-8 (processados normalmente)", reg["nomes_com_bytes_invalidos_em_utf8"]],
                ["Alterados durante a leitura", reg["alterados_durante_leitura"]],
                ["Abertura", reg["abertura"]],
                ["Tempo por fase (s)", str(reg["tempo_por_fase_s"])],
            ],
        )
    )
    M.append("\nO registro por arquivo esta em `registro_leitura_<etapa>.jsonl` (um objeto por linha: identificador, extensao, classe, tamanho, bytes lidos por fase, status).\n")
    M.append("\n## 9. Limites e o que nao sabemos\n")
    for x in perfil["limites_e_o_que_nao_sabemos"]:
        M.append("- " + x)
    M.append("")
    return "\n".join(M)


def limpar(o):
    if isinstance(o, dict):
        return {str(k): limpar(v) for k, v in o.items()}
    if isinstance(o, (list, tuple)):
        return [limpar(x) for x in o]
    if isinstance(o, np.ndarray):
        return limpar(o.tolist())
    if isinstance(o, (np.integer,)):
        return int(o)
    if isinstance(o, (np.floating, float)):
        x = float(o)
        return x if math.isfinite(x) else None
    if isinstance(o, np.bool_):
        return bool(o)
    return o


def abrir_saida(caminho, nome, binario=False):
    fd = os.open(os.path.join(caminho, nome), os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    return os.fdopen(fd, "wb" if binario else "w", encoding=None if binario else "utf-8")


def preparar_saida(caminho):
    os.makedirs(caminho, mode=0o700, exist_ok=True)
    if os.path.islink(caminho) or not os.path.isdir(caminho):
        raise ErroUso("--saida precisa ser um diretorio (nao um link simbolico)")


def escrever_json(saida, nome, obj):
    with abrir_saida(saida, nome) as f:
        json.dump(limpar(obj), f, indent=1, ensure_ascii=True, allow_nan=False)


def escrever_texto(saida, nome, texto):
    with abrir_saida(saida, nome) as f:
        f.write(texto)


def escrever_npz(saida, nome, arrays):
    with abrir_saida(saida, nome, True) as f:
        np.savez_compressed(f, **{k.replace("/", "__"): v for k, v in arrays.items()})


def escrever_registro(est, saida, etapa):
    with abrir_saida(saida, "registro_leitura_%s.jsonl" % etapa) as f:
        for item in linhas_registro(est):
            f.write(json.dumps(limpar(item), ensure_ascii=True) + "\n")


def ler_json(saida, nome):
    caminho = os.path.join(saida, nome)
    if not os.path.exists(caminho):
        raise ErroUso("falta %s em --saida (rode a etapa anterior)" % nome)
    with open(caminho) as f:
        return json.load(f)


def ler_npz(saida, nome):
    caminho = os.path.join(saida, nome)
    if not os.path.exists(caminho):
        return {}
    z = np.load(caminho, allow_pickle=False)
    return {k.replace("__", "/"): z[k] for k in z.files}


def recarregar_para_estimar(opc, var, amo, arr_var, arr_amo):
    opc.semente = amo["meta"]["semente"]
    opc.reamostras = amo["meta"]["opcoes"]["reamostras_bootstrap"]
    opc.min_arquivos_confianca = amo["meta"]["opcoes"]["min_arquivos_confianca"]
    est = Estado(opc)
    est.arrays = {}
    for ci, c in enumerate(CLASSES):
        if "amostra/" + c in arr_amo:
            meta = arr_amo["amostra_meta/" + c]
            plano_c = {"arquivos": int(meta[0]), "bytes": int(meta[1]), "censo": amo["por_classe"][c]["censo"], "pi": amo["por_classe"][c]["probabilidade_de_inclusao"]}
            resumir_classe(est, c, ci, arr_amo["amostra/" + c], plano_c, dict.fromkeys(CODECS, 0.0))
    vetores = {}
    for ci, c in enumerate(CLASSES):
        if "delta/" + c in arr_var:
            pares = [tuple(int(x) for x in l) for l in arr_var["delta/" + c]]
            _, vet = resumo_delta_classe(pares, np.random.default_rng([opc.semente, 404, ci]), opc.reamostras)
            vetores[c] = vet
    if "delta/_todos" in arr_var:
        pares = [tuple(int(x) for x in l) for l in arr_var["delta/_todos"]]
        _, vet = resumo_delta_classe(pares, np.random.default_rng([opc.semente, 405]), opc.reamostras)
        vetores["_todos"] = vet
    est.delta_vetores = vetores
    return est


def montar_parser():
    ap = argparse.ArgumentParser(prog="perfil_dataset.py", description="Perfil de dataset somente leitura: inventario, classificacao, dedup, familias de versoes, compressibilidade por classe e estimativa economica")
    sp = ap.add_subparsers(dest="cmd", required=True)
    ajuda = {
        "varrer": "inventario, classificacao, dedup de arquivo inteiro e familias de versoes (com delta medido)",
        "amostrar": "compressibilidade por classe em blocos de 1 MiB sorteados",
        "estimar": "conversao economica (le varredura.json e amostra.json de --saida)",
        "tudo": "varrer + amostrar + estimar + relatorio",
        "relatorio": "monta perfil.json e perfil.md a partir dos JSON de --saida",
        "autoteste": "testes da ferramenta (nao precisa de --raiz nem --saida)",
    }
    for nome in ajuda:
        p = sp.add_parser(nome, help=ajuda[nome])
        p.add_argument("--depurar", action="store_true", help="mostra o traceback completo em erros inesperados (pode conter nomes de arquivos do dataset)")
        if nome != "autoteste":
            if nome in ("varrer", "amostrar", "tudo"):
                p.add_argument("--raiz", required=True, help="diretorio a perfilar (so leitura)")
            p.add_argument("--saida", required=True, help="unico lugar onde a ferramenta escreve")
        if nome in ("estimar", "tudo"):
            p.add_argument("--config", help="JSON com os parametros do cliente (valor, unidade, fonte, tipo)")
        if nome in ("varrer", "amostrar", "tudo"):
            p.add_argument("--amostra-mib", type=int, default=None, help="MiB amostrados por classe para codecs rapidos (padrao 256; 16 com --rapido)")
            p.add_argument("--amostra-mib-lento", type=int, default=None, help="MiB amostrados por classe para zstd-19, xz-9e e bzip2-9 (padrao 32; 4 com --rapido)")
            p.add_argument("--max-arquivos", type=int, default=0, help="para o inventario depois de N arquivos (0 = sem limite)")
            p.add_argument("--hash-completo", action="store_true", help="hash completo de todos os candidatos por tamanho, sem o pre-filtro por hash de amostra")
            p.add_argument("--rapido", action="store_true", help="amostras e janelas menores, bootstrap com 200 reamostras")
            p.add_argument("--revelar-nomes", action="store_true", help="inclui nomes e caminhos na saida (padrao: anonimizada)")
            p.add_argument("--semente", type=int, default=1)
            p.add_argument("--processos", type=int, default=4, help="trabalhadores (no maximo %d: com o principal, ate %d processos)" % (MAX_PROCESSOS, MAX_PROCESSOS + 1))
            p.add_argument("--max-pares", type=int, default=None, help="pares de versoes medidos com --patch-from (padrao 200; 20 com --rapido)")
            p.add_argument("--delta-mib", type=int, default=None, help="janela por arquivo na medida de delta (padrao 16; 4 com --rapido)")
            p.add_argument("--prof-max", type=int, default=64)
            p.add_argument("--reamostras", type=int, default=None, help="reamostras do bootstrap (padrao 1000; 200 com --rapido)")
            p.add_argument("--min-arquivos-confianca", type=int, default=5)
            p.add_argument("--tol-tamanho", type=float, default=0.5, help="tolerancia de tamanho entre versoes consecutivas de uma familia")
            p.add_argument("--max-unidades", type=int, default=200000, help="teto de blocos amostrados por classe")
            p.add_argument("--cruzar-dispositivos", action="store_true", help="entra em pontos de montagem dentro da raiz")
        if nome in ("estimar", "relatorio"):
            p.add_argument("--revelar-nomes", action="store_true", help=argparse.SUPPRESS)
        if nome == "autoteste":
            p.add_argument("--filtro", default="", help="roda so os testes cujo nome contem este texto")
    return ap


def opc_de_args(a):
    campos = {k: getattr(a, k) for k in OPC_PADRAO if hasattr(a, k)}
    return Opc(**campos)


def cmd_varrer(a):
    opc = opc_de_args(a)
    est = Estado(opc)
    try:
        preparar_saida(a.saida)
        est.abrir()
        var = etapa_varrer(est)
        escrever_json(a.saida, "varredura.json", var)
        escrever_npz(a.saida, "dados_varredura.npz", {k: v for k, v in est.arrays.items() if k.startswith("delta/")})
        escrever_registro(est, a.saida, "varrer")
    finally:
        est.fechar()
    print("varrer ok: %d arquivos, %s; saida em %s" % (var["inventario"]["n_arquivos"], f_bytes(var["inventario"]["bytes"]), a.saida))
    return 0


def cmd_amostrar(a):
    opc = opc_de_args(a)
    est = Estado(opc)
    try:
        preparar_saida(a.saida)
        est.abrir()
        amo = etapa_amostrar(est)
        escrever_json(a.saida, "amostra.json", amo)
        escrever_npz(a.saida, "dados_amostra.npz", {k: v for k, v in est.arrays.items() if k.startswith("amostra")})
        escrever_registro(est, a.saida, "amostrar")
    finally:
        est.fechar()
    z3 = (amo["dataset"] or {}).get("zstd-3")
    print("amostrar ok: %d classes; zstd-3 do dataset: %s; saida em %s" % (len(amo["por_classe"]), f_pct(z3["economia"]) if z3 else "-", a.saida))
    return 0


def cmd_estimar(a):
    params, avisos = carregar_params(a.config)
    var = ler_json(a.saida, "varredura.json")
    amo = ler_json(a.saida, "amostra.json")
    opc = Opc(saida=a.saida, revelar_nomes=getattr(a, "revelar_nomes", False))
    est = recarregar_para_estimar(opc, var, amo, ler_npz(a.saida, "dados_varredura.npz"), ler_npz(a.saida, "dados_amostra.npz"))
    eco = estimar_economia(est, var, amo, params, avisos, carregar_vazoes_medidas())
    preparar_saida(a.saida)
    escrever_json(a.saida, "estimativa.json", eco)
    t = eco["total"]["politica_recomendada_com_dedup_e_delta"]["liquido_usd_mes"]
    print("estimar ok: liquido %s/mes %s" % (f_usd(t["estimativa"]), f_ic(t["ic95"], f_usd)))
    return 0


def cmd_relatorio(a):
    var = ler_json(a.saida, "varredura.json")
    amo = ler_json(a.saida, "amostra.json")
    eco = None
    if os.path.exists(os.path.join(a.saida, "estimativa.json")):
        eco = ler_json(a.saida, "estimativa.json")
    perfil = montar_perfil(var, amo, eco)
    preparar_saida(a.saida)
    escrever_json(a.saida, "perfil.json", perfil)
    escrever_texto(a.saida, "perfil.md", renderizar_md(limpar(perfil)))
    print("relatorio ok: perfil.json e perfil.md em %s" % a.saida)
    return 0


def pipeline_tudo(est, params, avisos, saida=None):
    est.abrir()
    var = etapa_varrer(est)
    amo = etapa_amostrar(est)
    eco = estimar_economia(est, var, amo, params, avisos, carregar_vazoes_medidas())
    registro = montar_registro(est)
    perfil = montar_perfil(var, amo, eco, registro)
    if saida:
        escrever_json(saida, "varredura.json", var)
        escrever_json(saida, "amostra.json", amo)
        escrever_json(saida, "estimativa.json", eco)
        escrever_npz(saida, "dados_varredura.npz", {k: v for k, v in est.arrays.items() if k.startswith("delta/")})
        escrever_npz(saida, "dados_amostra.npz", {k: v for k, v in est.arrays.items() if k.startswith("amostra")})
        escrever_registro(est, saida, "tudo")
        escrever_json(saida, "perfil.json", perfil)
        escrever_texto(saida, "perfil.md", renderizar_md(limpar(perfil)))
    return var, amo, eco, perfil


def cmd_tudo(a):
    opc = opc_de_args(a)
    params, avisos = carregar_params(a.config)
    est = Estado(opc)
    try:
        preparar_saida(a.saida)
        var, amo, eco, perfil = pipeline_tudo(est, params, avisos, a.saida)
    finally:
        est.fechar()
    t = eco["total"]["politica_recomendada_com_dedup_e_delta"]["liquido_usd_mes"]
    print("tudo ok: %d arquivos, %s; liquido estimado %s/mes; saida em %s" % (var["inventario"]["n_arquivos"], f_bytes(var["inventario"]["bytes"]), f_usd(t["estimativa"]), a.saida))
    return 0


def cmd_autoteste(a):
    m = carregar_modulo("perfil_testes", os.path.join(BASE, "perfil_testes.py"))
    return m.executar(sys.modules[__name__], a.filtro)


def main(argv=None):
    ap = montar_parser()
    a = ap.parse_args(argv)
    try:
        return {"varrer": cmd_varrer, "amostrar": cmd_amostrar, "estimar": cmd_estimar, "relatorio": cmd_relatorio, "tudo": cmd_tudo, "autoteste": cmd_autoteste}[a.cmd](a)
    except ErroUso as e:
        print("erro: %s" % e, file=sys.stderr)
        return 2
    except Exception as e:
        if getattr(a, "depurar", False):
            raise
        print("erro inesperado (%s): detalhes suprimidos porque podem conter nomes do dataset; repita com --depurar" % type(e).__name__, file=sys.stderr)
        return 3


if __name__ == "__main__":
    sys.exit(main())
