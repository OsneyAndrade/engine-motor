import argparse
import bz2
import gc
import hashlib
import json
import lzma
import multiprocessing as mp
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import time
import zlib
from compression import zstd

MIB = 1 << 20
KIB = 1 << 10

CONFIGS_INTEIRO = [
    "stored",
    "zstd-1",
    "zstd-3",
    "zstd-9",
    "zstd-19",
    "zstd-22",
    "zstd-3-long27",
    "xz-1",
    "xz-6",
    "xz-9e",
    "bzip2-9",
    "zlib-1",
    "zlib-6",
    "zlib-9",
]
CONFIGS_BLOCO = {
    "bloco_1MiB": ["stored", "zstd-1", "zstd-3", "zstd-9", "zstd-19", "zstd-22", "xz-9e", "bzip2-9", "zlib-6"],
    "bloco_64KiB": ["stored", "zstd-1", "zstd-3", "zstd-9", "zstd-19", "xz-9e", "bzip2-9", "zlib-6"],
}
TAMANHO_BLOCO = {"inteiro": 0, "bloco_1MiB": MIB, "bloco_64KiB": 64 * KIB}
CONFIGS_COMPLETO = ["zstd-1", "zstd-3", "zstd-3-long27", "zstd-9"]
VELOCIDADE_ESTIMADA = {
    "stored": 5000,
    "zstd-1": 500,
    "zstd-3": 300,
    "zstd-9": 80,
    "zstd-19": 4,
    "zstd-22": 2.5,
    "zstd-3-long27": 250,
    "xz-1": 45,
    "xz-6": 7,
    "xz-9e": 3.5,
    "bzip2-9": 14,
    "zlib-1": 90,
    "zlib-6": 25,
    "zlib-9": 10,
}
CLASSES_DICIONARIO = {"texto", "codigo", "sint_logs", "xml"}
NIVEIS_DICIONARIO = [3, 9]
SUBCONJUNTO_ECORE = {
    "arquivos": ["dickens", "mozilla"],
    "modos": ["inteiro", "bloco_1MiB"],
    "configs": ["zstd-1", "zstd-3", "zstd-9", "zstd-19", "zlib-6", "xz-6"],
}
ORDEM_ESCALA = list(range(0, 16, 2)) + list(range(16, 32)) + list(range(1, 16, 2))


def fabricar(config, tamanho_bloco, zdict=None):
    if config == "stored":
        return (lambda d: bytes(memoryview(d))), (lambda c: bytes(memoryview(c)))
    if config.startswith("zstd-"):
        partes = config.split("-")
        nivel = int(partes[1])
        opc = None
        opd = None
        if len(partes) > 2 and partes[2] == "long27":
            opc = {
                zstd.CompressionParameter.compression_level: nivel,
                zstd.CompressionParameter.window_log: 27,
                zstd.CompressionParameter.enable_long_distance_matching: 1,
            }
            opd = {zstd.DecompressionParameter.window_log_max: 27}
            return (
                lambda d: zstd.compress(d, options=opc),
                lambda c: zstd.decompress(c, options=opd),
            )
        if zdict is not None:
            return (
                lambda d: zstd.compress(d, level=nivel, options=opc, zstd_dict=zdict.as_digested_dict),
                lambda c: zstd.decompress(c, options=opd, zstd_dict=zdict.as_digested_dict),
            )
        return (
            lambda d: zstd.compress(d, level=nivel, options=opc),
            lambda c: zstd.decompress(c, options=opd),
        )
    if config.startswith("xz-"):
        p = config[3:]
        preset = int(p[0]) | (lzma.PRESET_EXTREME if p.endswith("e") else 0)
        if tamanho_bloco:
            filtros = [{"id": lzma.FILTER_LZMA2, "preset": preset, "dict_size": max(tamanho_bloco, 4096)}]
            return (
                lambda d: lzma.compress(d, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64, filters=filtros),
                lzma.decompress,
            )
        return (lambda d: lzma.compress(d, format=lzma.FORMAT_XZ, check=lzma.CHECK_CRC64, preset=preset)), lzma.decompress
    if config.startswith("bzip2-"):
        nivel = int(config.split("-")[1])
        return (lambda d: bz2.compress(d, nivel)), bz2.decompress
    if config.startswith("zlib-"):
        nivel = int(config.split("-")[1])
        return (lambda d: zlib.compress(d, nivel)), zlib.decompress
    raise ValueError(config)


def dividir(dados, tamanho_bloco):
    if not tamanho_bloco:
        return [bytes(dados)]
    return [bytes(dados[i : i + tamanho_bloco]) for i in range(0, len(dados), tamanho_bloco)]


def sha256_blocos(blocos):
    h = hashlib.sha256()
    for b in blocos:
        h.update(b)
    return h.hexdigest()


def estatisticas(tempos, n_bytes):
    mbps = [n_bytes / 1e6 / t for t in tempos]
    return {
        "tempos_s": tempos,
        "mediana_s": statistics.median(tempos),
        "MBps_mediana": statistics.median(mbps),
        "MBps_min": min(mbps),
        "MBps_max": max(mbps),
    }


def medir(comp, decomp, blocos, repeticoes):
    n_bytes = sum(len(b) for b in blocos)
    ref = sha256_blocos(blocos)
    aq = blocos[0][: min(len(blocos[0]), MIB)]
    decomp(comp(aq))
    tc = []
    saidas = None
    for _ in range(repeticoes):
        t0 = time.perf_counter_ns()
        saidas = [comp(b) for b in blocos]
        t1 = time.perf_counter_ns()
        tc.append((t1 - t0) / 1e9)
    n_comp = sum(len(c) for c in saidas)
    td = []
    exato = True
    for _ in range(repeticoes):
        t0 = time.perf_counter_ns()
        rec = [decomp(c) for c in saidas]
        t1 = time.perf_counter_ns()
        td.append((t1 - t0) / 1e9)
        if sha256_blocos(rec) != ref:
            exato = False
    return {
        "bytes_orig": n_bytes,
        "bytes_comp": n_comp,
        "fracao_economizada": 1.0 - n_comp / n_bytes,
        "n_blocos": len(blocos),
        "comp": estatisticas(tc, n_bytes),
        "decomp": estatisticas(td, n_bytes),
        "bit_exato": exato,
        "sha256_original": ref,
    }


def ler_fatia(raiz, caminho, cap, deslocamento=0):
    with open(os.path.join(raiz, caminho), "rb") as f:
        f.seek(deslocamento)
        return f.read(cap)


def celula_principal(cel, raiz, repeticoes):
    dados = ler_fatia(raiz, cel["caminho"], cel["cap"])
    blocos = dividir(dados, TAMANHO_BLOCO[cel["modo"]])
    comp, decomp = fabricar(cel["config"], TAMANHO_BLOCO[cel["modo"]])
    r = medir(comp, decomp, blocos, repeticoes)
    r.update({"tipo": "principal", "arquivo": cel["arquivo"], "classe": cel["classe"], "modo": cel["modo"], "config": cel["config"]})
    return r


def celula_completa(cel, raiz, repeticoes):
    dados = ler_fatia(raiz, cel["caminho"], -1)
    comp, decomp = fabricar(cel["config"], 0)
    r = medir(comp, decomp, [dados], repeticoes)
    r.update({"tipo": "completo", "arquivo": cel["arquivo"], "classe": cel["classe"], "modo": "inteiro_arquivo_completo", "config": cel["config"]})
    return r


def celula_dicionario(cel, raiz, repeticoes):
    bloco = 64 * KIB
    if cel["treino"] is None:
        dados = ler_fatia(raiz, cel["caminho"], cel["regiao"])
        corte = (int(len(dados) * 0.4) // bloco) * bloco
        treino = dados[:corte]
        teste = dados[corte:]
    else:
        treino = ler_fatia(raiz, cel["treino"], 8 * MIB)
        teste = ler_fatia(raiz, cel["caminho"], cel["regiao"])
    amostras = dividir(treino, bloco)
    if len(amostras[-1]) < bloco:
        amostras = amostras[:-1]
    pasta = os.path.join(raiz, "_dicionarios", cel["arquivo"])
    os.makedirs(os.path.join(pasta, "amostras"), exist_ok=False)
    caminhos = []
    for i, a in enumerate(amostras):
        p = os.path.join(pasta, "amostras", "a%05d.bin" % i)
        with open(p, "wb") as f:
            f.write(a)
        caminhos.append(p)
    arq_dic = os.path.join(pasta, "dic.bin")
    t0 = time.perf_counter()
    subprocess.run(
        ["zstd", "--train", "-q", "-T1", "--maxdict=110000", "-o", arq_dic] + caminhos,
        check=True,
        capture_output=True,
    )
    t_treino = time.perf_counter() - t0
    with open(arq_dic, "rb") as f:
        bytes_dic = f.read()
    zd = zstd.ZstdDict(bytes_dic)
    blocos = dividir(teste, bloco)
    saida = []
    for nivel in NIVEIS_DICIONARIO:
        for com_dic in (False, True):
            comp, decomp = fabricar("zstd-%d" % nivel, bloco, zd if com_dic else None)
            r = medir(comp, decomp, blocos, repeticoes)
            r.update(
                {
                    "tipo": "dicionario",
                    "arquivo": cel["arquivo"],
                    "classe": cel["classe"],
                    "modo": "bloco_64KiB",
                    "config": "zstd-%d" % nivel,
                    "com_dicionario": com_dic,
                    "tamanho_dicionario": len(bytes_dic),
                    "n_amostras_treino": len(amostras),
                    "bytes_treino": len(treino),
                    "bytes_teste": len(teste),
                    "tempo_treino_s": t_treino,
                    "sha256_dicionario": hashlib.sha256(bytes_dic).hexdigest(),
                }
            )
            saida.append(r)
    shutil.rmtree(os.path.join(pasta, "amostras"))
    return saida


def trabalhador(cpu, fila, resultados, raiz, repeticoes):
    gc.disable()
    os.sched_setaffinity(0, {cpu})
    while True:
        cel = fila.get()
        if cel is None:
            break
        try:
            if cel["tipo"] == "principal":
                r = celula_principal(cel, raiz, repeticoes)
            elif cel["tipo"] == "completo":
                r = celula_completa(cel, raiz, repeticoes)
            else:
                r = celula_dicionario(cel, raiz, repeticoes)
            if isinstance(r, dict):
                r = [r]
            for x in r:
                x["cpu"] = cpu
            resultados.put(("ok", cel.get("arquivo"), r))
        except Exception as e:
            resultados.put(("erro", cel.get("arquivo"), repr(e), {k: v for k, v in cel.items() if k != "caminho"}))


def executar_celulas(celulas, cpus, raiz, repeticoes, rotulo):
    ctx = mp.get_context("fork")
    fila = ctx.Queue()
    resultados = ctx.Queue()
    celulas = sorted(celulas, key=lambda c: -c["custo"])
    for c in celulas:
        fila.put(c)
    for _ in cpus:
        fila.put(None)
    procs = [ctx.Process(target=trabalhador, args=(c, fila, resultados, raiz, repeticoes)) for c in cpus]
    for p in procs:
        p.start()
    ok = []
    erros = []
    t0 = time.time()
    for i in range(len(celulas)):
        r = resultados.get()
        if r[0] == "ok":
            ok.extend(r[2])
        else:
            erros.append({"arquivo": r[1], "erro": r[2], "celula": r[3]})
        if (i + 1) % 25 == 0 or i + 1 == len(celulas):
            print("[%s] %d/%d celulas, %.0fs" % (rotulo, i + 1, len(celulas), time.time() - t0), file=sys.stderr, flush=True)
    for p in procs:
        p.join()
    return ok, erros


def custo_celula(modo, config, tamanho):
    v = VELOCIDADE_ESTIMADA[config]
    if modo != "inteiro" and config in ("zstd-19", "zstd-22", "xz-9e"):
        v *= 1.5
    return tamanho / 1e6 / v


def montar_celulas_principais(manifesto, cap):
    celulas = []
    for a in manifesto["arquivos"]:
        tam = min(a["tamanho"], cap)
        for modo, tb in TAMANHO_BLOCO.items():
            configs = CONFIGS_INTEIRO if modo == "inteiro" else CONFIGS_BLOCO[modo]
            for cfg in configs:
                celulas.append(
                    {
                        "tipo": "principal",
                        "arquivo": a["nome"],
                        "classe": a["classe"],
                        "caminho": a["caminho"],
                        "cap": cap,
                        "modo": modo,
                        "config": cfg,
                        "custo": custo_celula(modo, cfg, tam),
                    }
                )
    return celulas


def montar_celulas_completas(manifesto, cap):
    celulas = []
    for a in manifesto["arquivos"]:
        if a["tamanho"] > cap:
            for cfg in CONFIGS_COMPLETO:
                celulas.append(
                    {
                        "tipo": "completo",
                        "arquivo": a["nome"],
                        "classe": a["classe"],
                        "caminho": a["caminho"],
                        "config": cfg,
                        "custo": custo_celula("inteiro", cfg, a["tamanho"]),
                    }
                )
    return celulas


def montar_celulas_dicionario(manifesto, cap):
    celulas = []
    for a in manifesto["arquivos"]:
        if a["classe"] not in CLASSES_DICIONARIO:
            continue
        cel = {
            "tipo": "dicionario",
            "arquivo": a["nome"],
            "classe": a["classe"],
            "caminho": a["caminho"],
            "custo": 40.0,
        }
        if a["classe"] == "sint_logs":
            cel["treino"] = manifesto["treino_logs"]["caminho"]
            cel["regiao"] = cap
        else:
            cel["treino"] = None
            cel["regiao"] = min(a["tamanho"], 2 * cap)
        celulas.append(cel)
    return celulas


def celulas_ecore(manifesto, cap):
    celulas = []
    for a in manifesto["arquivos"]:
        if a["nome"] not in SUBCONJUNTO_ECORE["arquivos"]:
            continue
        for modo in SUBCONJUNTO_ECORE["modos"]:
            for cfg in SUBCONJUNTO_ECORE["configs"]:
                celulas.append(
                    {
                        "tipo": "principal",
                        "arquivo": a["nome"],
                        "classe": a["classe"],
                        "caminho": a["caminho"],
                        "cap": cap,
                        "modo": modo,
                        "config": cfg,
                        "custo": custo_celula(modo, cfg, min(a["tamanho"], cap)),
                    }
                )
    return celulas


def classificar_cpus():
    saida = subprocess.run(["lscpu", "-e=CPU,CORE,MAXMHZ"], capture_output=True, text=True).stdout.strip().splitlines()[1:]
    linhas = [(int(x.split()[0]), int(x.split()[1]), float(x.split()[2])) for x in saida]
    maximo = max(m for _, _, m in linhas)
    p = sorted(c for c, _, m in linhas if m >= 0.9 * maximo)
    e = sorted(c for c, _, m in linhas if m < 0.9 * maximo)
    return {"P_threads": p, "E_threads": e, "P_nucleos_fisicos": len({k for c, k, m in linhas if c in p}), "E_nucleos_fisicos": len({k for c, k, m in linhas if c in e})}


def versoes():
    def cli(cmd):
        r = subprocess.run(cmd, capture_output=True, text=True)
        return (r.stdout + r.stderr).strip().splitlines()[0]

    modelo = ""
    with open("/proc/cpuinfo") as f:
        for linha in f:
            if linha.startswith("model name"):
                modelo = linha.split(":", 1)[1].strip()
                break
    def ler(p):
        try:
            with open(p) as f:
                return f.read().strip()
        except OSError:
            return None

    return {
        "cpu_modelo": modelo,
        "kernel": platform.release(),
        "python": platform.python_version(),
        "libzstd_python": zstd.zstd_version,
        "zstd_cli": cli(["zstd", "--version"]),
        "xz_cli": cli(["xz", "--version"]),
        "liblzma_python": lzma.__name__ + " (liblzma do sistema)",
        "bzip2_cli": cli(["bzip2", "--help"]),
        "zlib_runtime": zlib.ZLIB_RUNTIME_VERSION,
        "governor_cpu0": ler("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        "intel_no_turbo": ler("/sys/devices/system/cpu/intel_pstate/no_turbo"),
        "topologia": classificar_cpus(),
    }


def energia():
    base = "/sys/class/powercap"
    try:
        for d in sorted(os.listdir(base)):
            p = os.path.join(base, d, "energy_uj")
            if os.path.exists(p):
                with open(p) as f:
                    f.read()
                return {"medida": True, "caminho": p}
    except PermissionError as e:
        return {"medida": False, "motivo": "energia nao medida: energy_uj sem permissao de leitura sem sudo (%s)" % e}
    except OSError as e:
        return {"medida": False, "motivo": "energia nao medida: " + repr(e)}
    return {"medida": False, "motivo": "energia nao medida: RAPL ausente"}


def carregar_saida(caminho):
    if os.path.exists(caminho):
        with open(caminho) as f:
            return json.load(f)
    return {}


def salvar_saida(caminho, obj):
    tmp = caminho + ".tmp"
    with open(tmp, "w") as f:
        json.dump(obj, f, indent=1)
    os.replace(tmp, caminho)


def fase_memoria(raiz, cpu, saida):
    pasta = os.path.join(raiz, "_memoria")
    os.makedirs(pasta, exist_ok=True)
    moz = next(a for a in saida["arquivos"] if a["nome"] in ("mozilla", "local_binarios.bin"))
    entrada = os.path.join(pasta, "entrada_32MiB.bin")
    with open(entrada, "wb") as f:
        f.write(ler_fatia(raiz, moz["caminho"], 32 * MIB))
    casos = [
        ("zstd-3", ["zstd", "-3", "-T1", "-q", "-c"], ["zstd", "-d", "-q", "-c"]),
        ("zstd-9", ["zstd", "-9", "-T1", "-q", "-c"], ["zstd", "-d", "-q", "-c"]),
        ("zstd-19", ["zstd", "-19", "-T1", "-q", "-c"], ["zstd", "-d", "-q", "-c"]),
        ("zstd-22", ["zstd", "--ultra", "-22", "-T1", "-q", "-c"], ["zstd", "-d", "-q", "-c"]),
        ("zstd-3-long27", ["zstd", "-3", "--long=27", "-T1", "-q", "-c"], ["zstd", "-d", "--long=27", "-q", "-c"]),
        ("xz-9e", ["xz", "-9e", "-T1", "-q", "-c"], ["xz", "-d", "-q", "-c"]),
    ]
    padrao = re.compile(r"Maximum resident set size \(kbytes\): (\d+)")
    elap = re.compile(r"Elapsed \(wall clock\) time.*: ([\d:.]+)")
    res = []
    for nome, cmd_c, cmd_d in casos:
        comprimido = os.path.join(pasta, nome + ".cmp")
        linha = {"config": nome, "entrada_bytes": os.path.getsize(entrada)}
        with open(comprimido, "wb") as out:
            r = subprocess.run(["/usr/bin/time", "-v", "taskset", "-c", str(cpu)] + cmd_c + [entrada], stdout=out, stderr=subprocess.PIPE, text=True)
        linha["rss_comp_kb"] = int(padrao.search(r.stderr).group(1))
        linha["comprimido_bytes"] = os.path.getsize(comprimido)
        linha["wall_comp"] = elap.search(r.stderr).group(1)
        r = subprocess.run(["/usr/bin/time", "-v", "taskset", "-c", str(cpu)] + cmd_d + [comprimido], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
        linha["rss_decomp_kb"] = int(padrao.search(r.stderr).group(1))
        res.append(linha)
        print("[memoria]", linha, file=sys.stderr, flush=True)
    return res


def fase_cli_referencia(raiz, cpu, saida):
    alvos = [a for a in saida["arquivos"] if a["nome"] in ("dickens", "mozilla", "local_texto.bin", "local_binarios.bin")]
    res = []
    padrao = re.compile(r"->\s+(\d+)\s+\(x([\d.]+)\),\s+([\d.]+) MB/s,\s+([\d.]+) MB/s")
    for a in alvos:
        pasta = os.path.join(raiz, "_cli_ref")
        os.makedirs(pasta, exist_ok=True)
        arq = os.path.join(pasta, a["nome"] + ".fatia")
        with open(arq, "wb") as f:
            f.write(ler_fatia(raiz, a["caminho"], saida["protocolo"]["cap_bytes"]))
        for nivel, bloco in ((1, 0), (3, 0), (3, MIB)):
            cmd = ["taskset", "-c", str(cpu), "zstd", "-b%d" % nivel, "-i2"] + (["-B%d" % bloco] if bloco else []) + [arq]
            r = subprocess.run(cmd, capture_output=True, text=True)
            texto = (r.stdout + r.stderr).replace("\r", "\n")
            m = None
            for m in padrao.finditer(texto):
                pass
            if m:
                res.append(
                    {
                        "arquivo": a["nome"],
                        "nivel": nivel,
                        "bloco": bloco,
                        "bytes_comp": int(m.group(1)),
                        "comp_MBps": float(m.group(3)),
                        "decomp_MBps": float(m.group(4)),
                    }
                )
    return res


def pool_blocos(manifesto):
    blocos = []
    for a in manifesto["arquivos"]:
        if a["sintetico"]:
            continue
        for off in range(0, a["tamanho"] - MIB + 1, MIB):
            blocos.append((a["caminho"], off))
    return blocos


def trabalhador_escala(i, cpu, n, barreira, resultados, raiz, indices, config, duracao, rodadas):
    gc.disable()
    os.sched_setaffinity(0, {cpu})
    dados = []
    for j in indices:
        c, off = j
        dados.append(ler_fatia(raiz, c, MIB, off))
    comp, decomp = fabricar(config, MIB)
    comprimidos = [comp(b) for b in dados]
    rec = [decomp(c) for c in comprimidos]
    exato = sha256_blocos(rec) == sha256_blocos(dados)
    resultados.put(("setup", i, exato, sum(len(c) for c in comprimidos), sum(len(b) for b in dados)))
    for op in ("comp", "decomp"):
        for r in range(rodadas):
            barreira.wait(timeout=300)
            t0 = time.perf_counter()
            fim = t0 + duracao
            n_bytes = 0
            if op == "comp":
                while True:
                    for b in dados:
                        comp(b)
                        n_bytes += len(b)
                        if time.perf_counter() >= fim:
                            break
                    if time.perf_counter() >= fim:
                        break
            else:
                while True:
                    for k, c in enumerate(comprimidos):
                        decomp(c)
                        n_bytes += len(dados[k])
                        if time.perf_counter() >= fim:
                            break
                    if time.perf_counter() >= fim:
                        break
            dt = time.perf_counter() - t0
            resultados.put(("medida", i, op, r, n_bytes, dt))


def tempos_isolados(raiz, pool, config, cpu, repeticoes):
    os.sched_setaffinity(0, {cpu})
    comp, decomp = fabricar(config, MIB)
    dados = [ler_fatia(raiz, c, MIB, off) for c, off in pool]
    comprimidos = [comp(b) for b in dados]
    tc = [[] for _ in dados]
    td = [[] for _ in dados]
    for _ in range(repeticoes + 1):
        for k, b in enumerate(dados):
            t0 = time.perf_counter_ns()
            comp(b)
            tc[k].append((time.perf_counter_ns() - t0) / 1e9)
        for k, c in enumerate(comprimidos):
            t0 = time.perf_counter_ns()
            decomp(c)
            td[k].append((time.perf_counter_ns() - t0) / 1e9)
    return [statistics.median(x[1:]) for x in tc], [statistics.median(x[1:]) for x in td], sum(len(c) for c in comprimidos), sum(len(b) for b in dados)


def fase_escala(raiz, manifesto, valores_n, repeticoes, duracao, cpu_base):
    pool = pool_blocos(manifesto)
    ctx = mp.get_context("fork")
    saida = {"duracao_s": duracao, "ordem_cpus": ORDEM_ESCALA, "n_blocos_pool": len(pool), "bloco_bytes": MIB, "configs": {}}
    max_n = max(valores_n)
    for config in ("zstd-3", "zstd-1"):
        tc, td, n_comp_pool, n_pool = tempos_isolados(raiz, pool, config, cpu_base, repeticoes)
        res_cfg = {"pool_razao_fracao_economizada": 1 - n_comp_pool / n_pool, "por_n": []}
        print("[escala] %s pool isolado: comp %.0f MB/s decomp %.0f MB/s" % (config, n_pool / 1e6 / sum(tc), n_pool / 1e6 / sum(td)), file=sys.stderr, flush=True)
        for n in valores_n:
            indices_por_worker = [[pool[j] for j in range(i, len(pool), max_n)] for i in range(n)]
            ideal = {"comp": 0.0, "decomp": 0.0}
            for i in range(n):
                ids = list(range(i, len(pool), max_n))
                b = len(ids) * MIB
                ideal["comp"] += b / 1e6 / sum(tc[j] for j in ids)
                ideal["decomp"] += b / 1e6 / sum(td[j] for j in ids)
            barreira = ctx.Barrier(n)
            resultados = ctx.Queue()
            procs = [
                ctx.Process(
                    target=trabalhador_escala,
                    args=(i, ORDEM_ESCALA[i], n, barreira, resultados, raiz, indices_por_worker[i], config, duracao, repeticoes + 1),
                )
                for i in range(n)
            ]
            for p in procs:
                p.start()
            exato = True
            n_msgs = n + n * 2 * (repeticoes + 1)
            medidas = {"comp": {}, "decomp": {}}
            for _ in range(n_msgs):
                m = resultados.get(timeout=600)
                if m[0] == "setup":
                    exato = exato and m[2]
                else:
                    _, i, op, r, nb, dt = m
                    medidas[op].setdefault(r, []).append(nb / 1e6 / dt)
            for p in procs:
                p.join()
            linha = {"n": n, "cpus": ORDEM_ESCALA[:n], "bit_exato": exato}
            for op in ("comp", "decomp"):
                agg = [sum(v) for r, v in sorted(medidas[op].items()) if r > 0]
                linha[op] = {
                    "agregado_MBps_por_repeticao": agg,
                    "agregado_MBps_mediana": statistics.median(agg),
                    "agregado_MBps_min": min(agg),
                    "agregado_MBps_max": max(agg),
                    "ideal_MBps_sem_contencao": ideal[op],
                    "eficiencia": statistics.median(agg) / ideal[op],
                    "por_nucleo_MBps_mediana": statistics.median(agg) / n,
                }
            res_cfg["por_n"].append(linha)
            print("[escala] %s N=%d comp %.0f MB/s (ef %.2f) decomp %.0f MB/s (ef %.2f)" % (config, n, linha["comp"]["agregado_MBps_mediana"], linha["comp"]["eficiencia"], linha["decomp"]["agregado_MBps_mediana"], linha["decomp"]["eficiencia"]), file=sys.stderr, flush=True)
        saida["configs"][config + "_bloco_1MiB"] = res_cfg
    return saida


def analisar_cpus(texto):
    cpus = []
    for parte in texto.split(","):
        parte = parte.strip()
        if "-" in parte:
            a, b = parte.split("-")
            cpus.extend(range(int(a), int(b) + 1))
        elif parte:
            cpus.append(int(parte))
    if len(set(cpus)) != len(cpus):
        raise ValueError("CPU repetida em " + texto)
    return cpus


def analisar_escala_cpus(texto, ns_padrao):
    grupos = []
    for item in texto.split(";"):
        item = item.strip()
        if not item:
            continue
        rotulo, resto = item.split("=", 1)
        if "@" in resto:
            lista, ns = resto.split("@", 1)
            valores_n = [int(x) for x in ns.split(",")]
        else:
            lista = resto
            valores_n = list(ns_padrao)
        cpus = analisar_cpus(lista)
        if max(valores_n) > len(cpus):
            raise ValueError("N maior que o numero de CPUs em " + item)
        grupos.append({"rotulo": rotulo.strip(), "cpus": cpus, "ns": valores_n})
    return grupos


def ideal_por_trabalhador(tc, td, ids):
    b = len(ids) * MIB
    return b / 1e6 / sum(tc[j] for j in ids), b / 1e6 / sum(td[j] for j in ids)


def fase_escala_cpus(raiz, manifesto, grupos, repeticoes, duracao, cpu_p, cpu_e):
    pool = pool_blocos(manifesto)
    ctx = mp.get_context("fork")
    topo = classificar_cpus()
    e_threads = set(topo["E_threads"])
    stride = max(32, max(max(g["ns"]) for g in grupos))
    saida = {
        "duracao_s": duracao,
        "n_blocos_pool": len(pool),
        "bloco_bytes": MIB,
        "stride_distribuicao_dados": stride,
        "cpu_referencia_P_isolado": cpu_p,
        "cpu_referencia_E_isolado": cpu_e,
        "grupos": {g["rotulo"]: {"cpus": g["cpus"], "ns": g["ns"], "configs": {}} for g in grupos},
        "isolado": {},
    }
    for config in ("zstd-3", "zstd-1"):
        iso = {}
        for tipo, cpu in (("P", cpu_p), ("E", cpu_e)):
            tc, td, n_comp_pool, n_pool = tempos_isolados(raiz, pool, config, cpu, repeticoes)
            iso[tipo] = (tc, td)
            saida["isolado"].setdefault(config + "_bloco_1MiB", {})[tipo] = {
                "cpu": cpu,
                "comp_MBps": n_pool / 1e6 / sum(tc),
                "decomp_MBps": n_pool / 1e6 / sum(td),
                "pool_razao_fracao_economizada": 1 - n_comp_pool / n_pool,
            }
            print("[escala-cpus] %s %s-core isolado (CPU %d): comp %.0f MB/s decomp %.0f MB/s" % (config, tipo, cpu, n_pool / 1e6 / sum(tc), n_pool / 1e6 / sum(td)), file=sys.stderr, flush=True)
        r = saida["isolado"][config + "_bloco_1MiB"]
        r["razao_E_sobre_P_comp"] = r["E"]["comp_MBps"] / r["P"]["comp_MBps"]
        r["razao_E_sobre_P_decomp"] = r["E"]["decomp_MBps"] / r["P"]["decomp_MBps"]
        for g in grupos:
            res_cfg = {"por_n": []}
            for n in g["ns"]:
                cpus_n = g["cpus"][:n]
                indices_por_worker = [[pool[j] for j in range(i, len(pool), stride)] for i in range(n)]
                ideal = {"comp": 0.0, "decomp": 0.0}
                ideal_p = {"comp": 0.0, "decomp": 0.0}
                for i in range(n):
                    ids = list(range(i, len(pool), stride))
                    tipo = "E" if cpus_n[i] in e_threads else "P"
                    c_t, d_t = ideal_por_trabalhador(iso[tipo][0], iso[tipo][1], ids)
                    c_p, d_p = ideal_por_trabalhador(iso["P"][0], iso["P"][1], ids)
                    ideal["comp"] += c_t
                    ideal["decomp"] += d_t
                    ideal_p["comp"] += c_p
                    ideal_p["decomp"] += d_p
                barreira = ctx.Barrier(n)
                resultados = ctx.Queue()
                procs = [
                    ctx.Process(
                        target=trabalhador_escala,
                        args=(i, cpus_n[i], n, barreira, resultados, raiz, indices_por_worker[i], config, duracao, repeticoes + 1),
                    )
                    for i in range(n)
                ]
                for p in procs:
                    p.start()
                exato = True
                n_msgs = n + n * 2 * (repeticoes + 1)
                medidas = {"comp": {}, "decomp": {}}
                for _ in range(n_msgs):
                    m = resultados.get(timeout=600)
                    if m[0] == "setup":
                        exato = exato and m[2]
                    else:
                        _, i, op, rr, nb, dt = m
                        medidas[op].setdefault(rr, []).append(nb / 1e6 / dt)
                for p in procs:
                    p.join()
                linha = {"n": n, "cpus": cpus_n, "n_E": sum(1 for c in cpus_n if c in e_threads), "n_P_threads": sum(1 for c in cpus_n if c not in e_threads), "bit_exato": exato}
                for op in ("comp", "decomp"):
                    agg = [sum(v) for rr, v in sorted(medidas[op].items()) if rr > 0]
                    med = statistics.median(agg)
                    linha[op] = {
                        "agregado_MBps_por_repeticao": agg,
                        "agregado_MBps_mediana": med,
                        "agregado_MBps_min": min(agg),
                        "agregado_MBps_max": max(agg),
                        "ideal_MBps_mesmo_tipo_de_nucleo_isolado": ideal[op],
                        "ideal_MBps_P_core_isolado": ideal_p[op],
                        "eficiencia_vs_mesmo_tipo": med / ideal[op],
                        "eficiencia_vs_P_core": med / ideal_p[op],
                        "por_nucleo_MBps_mediana": med / n,
                    }
                res_cfg["por_n"].append(linha)
                print("[escala-cpus] %s %s N=%d comp %.0f MB/s (ef %.2f) decomp %.0f MB/s (ef %.2f)" % (config, g["rotulo"], n, linha["comp"]["agregado_MBps_mediana"], linha["comp"]["eficiencia_vs_mesmo_tipo"], linha["decomp"]["agregado_MBps_mediana"], linha["decomp"]["eficiencia_vs_mesmo_tipo"]), file=sys.stderr, flush=True)
            saida["grupos"][g["rotulo"]]["configs"][config + "_bloco_1MiB"] = res_cfg
    return saida


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus-dir", required=True)
    ap.add_argument("--saida", required=True)
    ap.add_argument("--repeticoes", type=int, default=3)
    ap.add_argument("--cpu", default="0,2,4,6")
    ap.add_argument("--cpu-e", type=int, default=16)
    ap.add_argument("--cap-mib", type=int, default=8)
    ap.add_argument("--fase", default="tudo", choices=["tudo", "principal", "complemento", "dicionario", "ecore", "escala", "memoria"])
    ap.add_argument("--escala-n", default="1,2,4,8,16,24,32")
    ap.add_argument("--escala-duracao", type=float, default=1.5)
    ap.add_argument("--arquivos", default="")
    ap.add_argument("--escala-cpus", default="")
    a = ap.parse_args()
    if a.escala_cpus and (os.path.basename(a.saida) == "medicoes.json" or carregar_saida(a.saida).get("principal")):
        sys.exit("recusado: --escala-cpus nao pode sobrescrever uma saida com medicoes principais")

    raiz = os.path.abspath(a.corpus_dir)
    with open(os.path.join(raiz, "manifesto.json")) as f:
        manifesto = json.load(f)
    if a.arquivos:
        sel = set(a.arquivos.split(","))
        manifesto["arquivos"] = [x for x in manifesto["arquivos"] if x["nome"] in sel]
    cpus = [int(x) for x in a.cpu.split(",")]
    cap = a.cap_mib * MIB
    inicio = time.time()

    saida = carregar_saida(a.saida)
    saida.setdefault("falhas", [])
    saida["meta"] = versoes()
    saida["energia"] = energia()
    saida["protocolo"] = {
        "repeticoes": a.repeticoes,
        "cap_bytes": cap,
        "cpus_principal": cpus,
        "cpu_ecore": a.cpu_e,
        "mediana_de_repeticoes": True,
        "aquecimento": "compressao e descompressao de 1 MiB antes das repeticoes",
        "origem_dados": manifesto.get("origens"),
        "avisos_dados": manifesto.get("avisos"),
        "data_execucao": time.strftime("%Y-%m-%d %H:%M:%S"),
    }
    saida["arquivos"] = []
    for x in manifesto["arquivos"]:
        fatia = ler_fatia(raiz, x["caminho"], cap)
        saida["arquivos"].append(
            {
                "nome": x["nome"],
                "classe": x["classe"],
                "sintetico": x["sintetico"],
                "origem": x["origem"],
                "caminho": x["caminho"],
                "tamanho_bytes": x["tamanho"],
                "sha256_arquivo": x["sha256"],
                "tamanho_fatia_bytes": len(fatia),
                "sha256_fatia": hashlib.sha256(fatia).hexdigest(),
                "truncado_na_fatia": x["tamanho"] > cap,
            }
        )
    salvar_saida(a.saida, saida)

    fases = ["memoria", "principal", "dicionario", "ecore", "escala"] if a.fase == "tudo" else [a.fase]

    if "memoria" in fases:
        saida["memoria"] = fase_memoria(raiz, cpus[0], saida)
        saida["cli_referencia_zstd_bench"] = fase_cli_referencia(raiz, cpus[0], saida)
        salvar_saida(a.saida, saida)

    if "principal" in fases:
        celulas = montar_celulas_principais(manifesto, cap) + montar_celulas_completas(manifesto, cap)
        ok, erros = executar_celulas(celulas, cpus, raiz, a.repeticoes, "principal")
        saida["principal"] = [x for x in ok if x["tipo"] == "principal"]
        saida["completo"] = [x for x in ok if x["tipo"] == "completo"]
        saida["falhas"] = [f for f in saida["falhas"] if f.get("fase") != "principal"] + [dict(e, fase="principal") for e in erros]
        saida["bit_exatidao_violada"] = [
            {"arquivo": x["arquivo"], "modo": x["modo"], "config": x["config"]} for x in ok if not x["bit_exato"]
        ]
        salvar_saida(a.saida, saida)

    if "complemento" in fases:
        celulas = [c for c in montar_celulas_principais(manifesto, cap) if c["config"] == "zstd-3-long27"] + montar_celulas_completas(manifesto, cap)
        ok, erros = executar_celulas(celulas, cpus, raiz, a.repeticoes, "complemento")
        saida["principal"] = [x for x in saida.get("principal", []) if x["config"] != "zstd-3-long27"] + [x for x in ok if x["tipo"] == "principal"]
        saida["completo"] = [x for x in ok if x["tipo"] == "completo"]
        saida["falhas"] = [f for f in saida["falhas"] if f.get("fase") != "principal"] + [dict(e, fase="complemento") for e in erros]
        saida["bit_exatidao_violada"] = saida.get("bit_exatidao_violada", []) + [
            {"arquivo": x["arquivo"], "modo": x["modo"], "config": x["config"]} for x in ok if not x["bit_exato"]
        ]
        salvar_saida(a.saida, saida)

    if "dicionario" in fases:
        celulas = montar_celulas_dicionario(manifesto, cap)
        shutil.rmtree(os.path.join(raiz, "_dicionarios"), ignore_errors=True)
        os.makedirs(os.path.join(raiz, "_dicionarios"))
        ok, erros = executar_celulas(celulas, cpus, raiz, a.repeticoes, "dicionario")
        saida["dicionario"] = ok
        saida["falhas"] = [f for f in saida["falhas"] if f.get("fase") != "dicionario"] + [dict(e, fase="dicionario") for e in erros]
        saida["bit_exatidao_violada"] = saida.get("bit_exatidao_violada", []) + [
            {"arquivo": x["arquivo"], "modo": x["modo"], "config": x["config"], "com_dicionario": x["com_dicionario"]} for x in ok if not x["bit_exato"]
        ]
        salvar_saida(a.saida, saida)

    if "ecore" in fases:
        resultado = []
        for rotulo, cpu in (("P_isolado", cpus[0]), ("E_isolado", a.cpu_e)):
            ok, erros = executar_celulas(celulas_ecore(manifesto, cap), [cpu], raiz, a.repeticoes, "ecore-" + rotulo)
            for x in ok:
                x["tipo_nucleo"] = rotulo
            resultado.extend(ok)
            saida["falhas"] = [f for f in saida["falhas"] if f.get("fase") != "ecore-" + rotulo] + [dict(e, fase="ecore-" + rotulo) for e in erros]
        saida["ecore"] = resultado
        saida["bit_exatidao_violada"] = saida.get("bit_exatidao_violada", []) + [
            {"arquivo": x["arquivo"], "modo": x["modo"], "config": x["config"], "tipo_nucleo": x["tipo_nucleo"]} for x in resultado if not x["bit_exato"]
        ]
        salvar_saida(a.saida, saida)

    if "escala" in fases and a.escala_cpus:
        grupos = analisar_escala_cpus(a.escala_cpus, [int(x) for x in a.escala_n.split(",")])
        saida["escala_cpus"] = fase_escala_cpus(raiz, manifesto, grupos, a.repeticoes, a.escala_duracao, cpus[0], a.cpu_e)
        for rot, g in saida["escala_cpus"]["grupos"].items():
            for cfg, v in g["configs"].items():
                for linha in v["por_n"]:
                    if not linha["bit_exato"]:
                        saida.setdefault("bit_exatidao_violada", []).append({"escala_cpus": rot, "config": cfg, "n": linha["n"]})
        salvar_saida(a.saida, saida)
    elif "escala" in fases:
        saida["escala"] = fase_escala(raiz, manifesto, [int(x) for x in a.escala_n.split(",")], a.repeticoes, a.escala_duracao, cpus[0])
        for cfg, v in saida["escala"]["configs"].items():
            for linha in v["por_n"]:
                if not linha["bit_exato"]:
                    saida.setdefault("bit_exatidao_violada", []).append({"escala": cfg, "n": linha["n"]})
        salvar_saida(a.saida, saida)

    saida["tempo_total_s"] = saida.get("tempo_total_s", 0) + time.time() - inicio
    salvar_saida(a.saida, saida)
    print("concluido em %.0fs; violacoes de bit-exatidao: %d; falhas: %d" % (time.time() - inicio, len(saida.get("bit_exatidao_violada", [])), len(saida["falhas"])), file=sys.stderr)


if __name__ == "__main__":
    main()
