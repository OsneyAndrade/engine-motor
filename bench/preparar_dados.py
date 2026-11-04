import argparse
import hashlib
import json
import os
import subprocess
import sys
import urllib.request
import zipfile

SILESIA = {
    "dickens": ("texto", 10192446),
    "mozilla": ("binario", 51220480),
    "mr": ("cientifico", 9970564),
    "nci": ("cientifico", 33553445),
    "ooffice": ("binario", 6152192),
    "osdb": ("banco_de_dados", 10085684),
    "reymont": ("texto", 6627202),
    "samba": ("codigo", 21606400),
    "sao": ("cientifico", 7251944),
    "webster": ("texto", 41458703),
    "x-ray": ("cientifico", 8474240),
    "xml": ("xml", 5345280),
}
CANTERBURY = {
    "bible.txt": ("texto", 4047392),
    "E.coli": ("dna", 4638690),
    "world192.txt": ("texto", 2473400),
}
URLS_SILESIA = [
    "https://sun.aei.polsl.pl//~sdeor/corpus/silesia.zip",
    "https://mattmahoney.net/dc/silesia.zip",
]
URLS_CANTERBURY = ["https://corpus.canterbury.ac.nz/resources/large.zip"]
LIMITE_BYTES = 80 * 1000 * 1000


def sha256_arquivo(caminho):
    h = hashlib.sha256()
    with open(caminho, "rb") as f:
        while True:
            bloco = f.read(1 << 22)
            if not bloco:
                break
            h.update(bloco)
    return h.hexdigest()


def baixar(urls, destino):
    for url in urls:
        try:
            with urllib.request.urlopen(url, timeout=120) as r, open(destino, "wb") as f:
                while True:
                    bloco = r.read(1 << 20)
                    if not bloco:
                        break
                    f.write(bloco)
            return url
        except Exception as e:
            print("falha em", url, e, file=sys.stderr)
    return None


def diretorio_novo(caminho):
    os.makedirs(caminho, exist_ok=True)
    if os.listdir(caminho):
        sys.exit("diretorio precisa estar vazio: " + caminho)


def extrair_whitelist(zip_path, esperados, destino):
    diretorio_novo(destino)
    z = zipfile.ZipFile(zip_path)
    existentes = {i.filename: i for i in z.infolist()}
    saida = []
    for nome, (classe, tamanho) in esperados.items():
        info = existentes.get(nome)
        if info is None:
            print("ausente no zip:", nome, file=sys.stderr)
            continue
        if info.file_size > LIMITE_BYTES:
            print("recusado por tamanho:", nome, file=sys.stderr)
            continue
        caminho = os.path.join(destino, nome)
        with z.open(info) as src, open(caminho, "wb") as dst:
            restante = info.file_size
            while restante > 0:
                bloco = src.read(min(1 << 20, restante))
                if not bloco:
                    break
                dst.write(bloco)
                restante -= len(bloco)
        saida.append((nome, classe, tamanho, caminho))
    return saida


def fallback_local(destino):
    diretorio_novo(destino)
    saida = []
    alvos = [
        ("local_binarios.bin", "binario", ["/usr/bin"], None, 32),
        ("local_codigo.bin", "codigo", ["/usr/lib"], (".py",), 16),
        ("local_texto.bin", "texto", ["/usr/share/doc"], None, 16),
    ]
    for nome, classe, raizes, exts, mib in alvos:
        caminho = os.path.join(destino, nome)
        total = 0
        with open(caminho, "wb") as out:
            for raiz in raizes:
                for dirpath, dirnames, arquivos in os.walk(raiz):
                    dirnames.sort()
                    for a in sorted(arquivos):
                        if exts and not a.endswith(exts):
                            continue
                        p = os.path.join(dirpath, a)
                        if os.path.islink(p) or not os.path.isfile(p):
                            continue
                        try:
                            with open(p, "rb") as f:
                                dados = f.read(4 << 20)
                        except OSError:
                            continue
                        out.write(dados)
                        total += len(dados)
                        if total >= mib << 20:
                            break
                    if total >= mib << 20:
                        break
        saida.append((nome, classe, os.path.getsize(caminho), caminho))
    return saida


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--destino", required=True)
    ap.add_argument("--zips", required=True)
    ap.add_argument("--forcar-fallback-local", action="store_true")
    a = ap.parse_args()
    os.makedirs(a.zips, exist_ok=True)
    raiz = os.path.abspath(a.destino)
    os.makedirs(raiz, exist_ok=True)
    arquivos = []
    avisos = []

    zip_s = os.path.join(a.zips, "silesia.zip")
    zip_c = os.path.join(a.zips, "large.zip")
    origem_s = origem_c = None
    if not a.forcar_fallback_local:
        if not os.path.exists(zip_s):
            origem_s = baixar(URLS_SILESIA, zip_s)
        else:
            origem_s = "existente:" + zip_s
        if not os.path.exists(zip_c):
            origem_c = baixar(URLS_CANTERBURY, zip_c)
        else:
            origem_c = "existente:" + zip_c

    if origem_s and os.path.exists(zip_s):
        for nome, classe, tam, caminho in extrair_whitelist(zip_s, SILESIA, os.path.join(raiz, "silesia")):
            arquivos.append((nome, classe, False, "silesia", caminho, tam))
    else:
        avisos.append("download do Silesia falhou ou foi pulado; usando dados reais locais")
        for nome, classe, tam, caminho in fallback_local(os.path.join(raiz, "local")):
            arquivos.append((nome, classe, False, "local", caminho, tam))

    if origem_c and os.path.exists(zip_c):
        for nome, classe, tam, caminho in extrair_whitelist(zip_c, CANTERBURY, os.path.join(raiz, "canterbury")):
            arquivos.append((nome, classe, False, "canterbury_large", caminho, tam))
    else:
        avisos.append("Canterbury Large nao obtido")

    textos = [(os.path.getsize(c), c) for n, cl, s, o, c, t in arquivos if cl == "texto"]
    fonte_texto = max(textos)[1]
    sint = os.path.join(raiz, "sinteticos")
    r = subprocess.run(
        [sys.executable, "-I", os.path.join(os.path.dirname(os.path.abspath(__file__)), "gerar_sinteticos.py"), "--saida", sint, "--texto-fonte", fonte_texto],
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        sys.exit("gerador sintetico falhou: " + r.stderr)
    receita = json.loads(r.stdout.strip().splitlines()[-1])
    arquivos.append(("aleatorio.bin", "sint_incompressivel", True, "sintetico: " + receita["aleatorio"], os.path.join(sint, "aleatorio.bin"), None))
    arquivos.append(("logs.log", "sint_logs", True, "sintetico: " + receita["logs"], os.path.join(sint, "logs.log"), None))
    arquivos.append(("ja_comprimido.xz", "sint_ja_comprimido", True, "sintetico: " + receita["ja_comprimido"], os.path.join(sint, "ja_comprimido.xz"), None))

    manifesto = {"raiz": raiz, "origens": {"silesia": origem_s, "canterbury": origem_c}, "avisos": avisos, "arquivos": [], "treino_logs": None}
    for nome, classe, sintetico, origem, caminho, tam_esperado in arquivos:
        tam = os.path.getsize(caminho)
        entrada = {
            "nome": nome,
            "classe": classe,
            "sintetico": sintetico,
            "origem": origem,
            "caminho": os.path.relpath(caminho, raiz),
            "tamanho": tam,
            "sha256": sha256_arquivo(caminho),
        }
        if tam_esperado is not None and tam != tam_esperado:
            entrada["aviso_tamanho"] = "esperado %d" % tam_esperado
            avisos.append("tamanho divergente em " + nome)
        manifesto["arquivos"].append(entrada)
    tl = os.path.join(sint, "logs_treino.log")
    manifesto["treino_logs"] = {"caminho": os.path.relpath(tl, raiz), "tamanho": os.path.getsize(tl), "sha256": sha256_arquivo(tl), "sintetico": True}
    with open(os.path.join(raiz, "manifesto.json"), "w") as f:
        json.dump(manifesto, f, indent=1)
    print("arquivos:", len(manifesto["arquivos"]), "avisos:", avisos)


if __name__ == "__main__":
    main()
