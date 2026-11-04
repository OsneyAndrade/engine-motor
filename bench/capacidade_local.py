import argparse
import array
import gc
import glob
import hashlib
import importlib.util
import json
import mmap
import multiprocessing as mp
import os
import re
import shutil
import statistics
import struct
import subprocess
import sys
import tarfile
import time

os.environ.setdefault("OPENBLAS_NUM_THREADS", "1")
os.environ.setdefault("OMP_NUM_THREADS", "1")

BASE = os.path.dirname(os.path.abspath(__file__))
MIB = 1 << 20
GIB = 1 << 30
TICK = os.sysconf("SC_CLK_TCK")
SEG_MES = 2592000.0
ALVO_MEDIA_MBPS = 5e15 / SEG_MES / 1e6
ALVO_PICO_MBPS = 4.0 * ALVO_MEDIA_MBPS
CPU_PAI = 31
CPUS_P = list(range(0, 16))
CPUS_E = list(range(16, 32))
CONFIGS = {
    "A": list(range(0, 16, 2)),
    "B": list(range(0, 16, 2)) + list(range(16, 24)),
    "C": list(range(32)),
}
REF_ZSTD1 = {"razao": 2.896, "comp_MBps": 510.0, "decomp_MBps": 1550.0}
CACHE_IO = os.path.expanduser("~/.cache/syntra-capacity")
DISCO = "nvme0n1"


def carregar_modulo(nome, arquivo):
    spec = importlib.util.spec_from_file_location(nome, arquivo)
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


SB = carregar_modulo("syntrabench", os.path.join(BASE, "syntrabench.py"))


def log(msg):
    print("[%s] %s" % (time.strftime("%H:%M:%S"), msg), file=sys.stderr, flush=True)


def ler(caminho):
    try:
        with open(caminho) as f:
            return f.read().strip()
    except OSError:
        return None


def ler_num(caminho):
    v = ler(caminho)
    try:
        return float(v)
    except (TypeError, ValueError):
        return None


def pb_mes(mbps):
    return mbps * SEG_MES / 1e9


def mediana(xs):
    return statistics.median(xs) if xs else None


def media(xs):
    xs = [x for x in xs if x is not None]
    return sum(xs) / len(xs) if xs else None


def percentil(xs, p):
    if not len(xs):
        return None
    ordenado = sorted(xs)
    k = min(len(ordenado) - 1, max(0, int(round(p / 100.0 * (len(ordenado) - 1)))))
    return ordenado[k]


def carregar_json(caminho):
    if os.path.exists(caminho):
        with open(caminho) as f:
            return json.load(f)
    return {}


def gravar_secao(caminho, chave, valor):
    obj = carregar_json(caminho)
    obj[chave] = valor
    SB.salvar_saida(caminho, obj)


def gravar_subsecao(caminho, chave, sub, valor):
    obj = carregar_json(caminho)
    obj.setdefault(chave, {})[sub] = valor
    SB.salvar_saida(caminho, obj)


def varrer_proc():
    tab = {}
    for nome in os.listdir("/proc"):
        if not nome.isdigit():
            continue
        try:
            with open("/proc/%s/stat" % nome) as f:
                s = f.read()
        except OSError:
            continue
        i = s.rfind(")")
        comm = s[s.find("(") + 1 : i]
        resto = s[i + 2 :].split()
        tab[int(nome)] = (comm, int(resto[1]), int(resto[11]) + int(resto[12]))
    return tab


def descendentes(tab, raiz):
    conjunto = {raiz}
    mudou = True
    while mudou:
        mudou = False
        for pid, (_, ppid, _) in tab.items():
            if ppid in conjunto and pid not in conjunto:
                conjunto.add(pid)
                mudou = True
    conjunto.discard(raiz)
    return conjunto


def ler_stat_cpus():
    busy = {}
    with open("/proc/stat") as f:
        for linha in f:
            if linha.startswith("cpu") and linha[3] != " ":
                p = linha.split()
                v = [int(x) for x in p[1:9]]
                busy[int(p[0][3:])] = v[0] + v[1] + v[2] + v[5] + v[6] + v[7]
    return busy


def ler_diskstats(nome):
    with open("/proc/diskstats") as f:
        for linha in f:
            p = linha.split()
            if p[2] == nome:
                return {"rd_setores": int(p[5]), "wr_setores": int(p[9]), "io_ticks": int(p[12]), "fila_ms": int(p[13])}
    return None


def ler_meminfo():
    d = {}
    with open("/proc/meminfo") as f:
        for linha in f:
            k, v = linha.split(":", 1)
            d[k] = int(v.split()[0])
    return d


class Amostrador:
    def __init__(self, cpus_ativas, raiz_pid):
        self.ativas = sorted(cpus_ativas)
        afetadas = set(cpus_ativas)
        for c in cpus_ativas:
            if c < 16:
                afetadas.add(c ^ 1)
        self.afetadas = sorted(afetadas)
        self.raiz = raiz_pid
        self.zonas = [(z, ler(z + "/type")) for z in sorted(glob.glob("/sys/class/thermal/thermal_zone*"))]
        self.coretemp = []
        self.nvme_temp = None
        for h in glob.glob("/sys/class/hwmon/hwmon*"):
            nome = ler(h + "/name")
            if nome == "coretemp":
                self.coretemp = sorted(glob.glob(h + "/temp*_input"))
            if nome == "nvme" and os.path.exists(h + "/temp1_input"):
                self.nvme_temp = h + "/temp1_input"
        self.rapl = None
        self.rapl_max = None
        for d in sorted(glob.glob("/sys/class/powercap/intel-rapl:0")):
            if ler(d + "/energy_uj") is not None:
                self.rapl = d + "/energy_uj"
                self.rapl_max = ler_num(d + "/max_energy_range_uj")
        self.prev = None
        self.t0 = None
        self.base_throttle = None

    def snap(self):
        t = time.perf_counter()
        tab = varrer_proc()
        cpus = ler_stat_cpus()
        thr = {"core_count": 0, "core_total_ms": 0, "pkg_count": None, "pkg_total_ms": None}
        for c in range(32):
            d = "/sys/devices/system/cpu/cpu%d/thermal_throttle/" % c
            v = ler_num(d + "core_throttle_count")
            if v is not None:
                thr["core_count"] += int(v)
            v = ler_num(d + "core_throttle_total_time_ms")
            if v is not None:
                thr["core_total_ms"] += int(v)
        d0 = "/sys/devices/system/cpu/cpu0/thermal_throttle/"
        thr["pkg_count"] = ler_num(d0 + "package_throttle_count")
        thr["pkg_total_ms"] = ler_num(d0 + "package_throttle_total_time_ms")
        return {
            "t": t,
            "tab": tab,
            "cpus": cpus,
            "thr": thr,
            "energia": ler_num(self.rapl) if self.rapl else None,
            "disco": ler_diskstats(DISCO),
        }

    def iniciar(self):
        self.prev = self.snap()
        self.t0 = self.prev["t"]
        self.base_throttle = self.prev["thr"]

    def amostrar(self):
        cur = self.snap()
        prev = self.prev
        dt = cur["t"] - prev["t"]
        own = descendentes(cur["tab"], self.raiz) | descendentes(prev["tab"], self.raiz)
        own_j = 0
        outros = {}
        total_outros = 0
        for pid, (comm, _, j) in cur["tab"].items():
            if pid == self.raiz:
                continue
            if pid in prev["tab"]:
                d = j - prev["tab"][pid][2]
            else:
                d = j if pid in own else 0
            if d < 0:
                d = 0
            if pid in own:
                own_j += d
            else:
                total_outros += d
                if d:
                    outros[comm] = outros.get(comm, 0) + d
        busy_todas = sum(cur["cpus"][c] - prev["cpus"][c] for c in cur["cpus"])
        busy_conj = sum(cur["cpus"][c] - prev["cpus"][c] for c in self.afetadas if c in cur["cpus"])
        def cores(j):
            return max(0.0, j / TICK / dt)
        top = sorted(outros.items(), key=lambda kv: -kv[1])[:3]
        freq = {}
        for c in range(32):
            v = ler_num("/sys/devices/system/cpu/cpu%d/cpufreq/scaling_cur_freq" % c)
            freq[c] = v / 1000.0 if v is not None else None
        def mf(lista):
            return media([freq[c] for c in lista])
        temps = {}
        for z, tipo in self.zonas:
            v = ler_num(z + "/temp")
            temps["%s:%s" % (os.path.basename(z), tipo)] = v / 1000.0 if v is not None else None
        pkg = None
        for k, v in temps.items():
            if k.endswith(":x86_pkg_temp"):
                pkg = v
        ct = [ler_num(p) for p in self.coretemp]
        ct = [x / 1000.0 for x in ct if x is not None]
        nv = ler_num(self.nvme_temp) if self.nvme_temp else None
        potencia = None
        if self.rapl and cur["energia"] is not None and prev["energia"] is not None:
            de = cur["energia"] - prev["energia"]
            if de < 0 and self.rapl_max:
                de += self.rapl_max
            potencia = de / 1e6 / dt
        disco = None
        if cur["disco"] and prev["disco"]:
            a, b = prev["disco"], cur["disco"]
            disco = {
                "leitura_MBps": (b["rd_setores"] - a["rd_setores"]) * 512 / 1e6 / dt,
                "escrita_MBps": (b["wr_setores"] - a["wr_setores"]) * 512 / 1e6 / dt,
                "util_pct": (b["io_ticks"] - a["io_ticks"]) / (dt * 10.0),
                "fila_media": (b["fila_ms"] - a["fila_ms"]) / (dt * 1000.0),
            }
        mem = ler_meminfo()
        lf = [c for c in self.ativas if c in CPUS_P]
        le = [c for c in self.ativas if c in CPUS_E]
        rec = {
            "t_s": cur["t"] - self.t0,
            "dt_s": dt,
            "bg_cores_proc": cores(total_outros),
            "bg_cores_stat": cores(busy_todas - own_j),
            "bg_no_conjunto_cores": cores(busy_conj - own_j),
            "own_cores": cores(own_j),
            "bg_top": [[c, cores(j)] for c, j in top],
            "freq_P_ativos_MHz": mf(lf) if lf else None,
            "freq_E_ativos_MHz": mf(le) if le else None,
            "freq_P_todos_MHz": mf(CPUS_P),
            "freq_E_todos_MHz": mf(CPUS_E),
            "temp_C": temps,
            "temp_pkg_C": pkg,
            "temp_core_max_C": max(ct) if ct else None,
            "temp_nvme_C": nv / 1000.0 if nv is not None else None,
            "throttle": cur["thr"],
            "potencia_W": potencia,
            "disco": disco,
            "mem_disponivel_MiB": mem["MemAvailable"] / 1024.0,
            "swap_usado_MiB": (mem["SwapTotal"] - mem["SwapFree"]) / 1024.0,
            "loadavg1": float(ler("/proc/loadavg").split()[0]),
        }
        self.prev = cur
        return rec


def bg_resumo(recs):
    if not recs:
        return None
    conj = [r["bg_no_conjunto_cores"] for r in recs]
    acum = {}
    for r in recs:
        for comm, v in r["bg_top"]:
            acum[comm] = acum.get(comm, 0.0) + v * r["dt_s"]
    top = sorted(acum.items(), key=lambda kv: -kv[1])[:5]
    return {
        "bg_no_conjunto_cores_medio": media(conj),
        "bg_no_conjunto_cores_max": max(conj),
        "bg_total_cores_medio": media([r["bg_cores_proc"] for r in recs]),
        "bg_total_cores_max": max(r["bg_cores_proc"] for r in recs),
        "amostras_com_bg_acima_de_0_5_core": sum(1 for x in conj if x > 0.5),
        "amostras_com_bg_acima_de_2_cores": sum(1 for x in conj if x > 2.0),
        "n_amostras": len(recs),
        "maiores_processos_de_fundo_core_segundos": [[c, round(v, 1)] for c, v in top],
        "loadavg1_max": max(r["loadavg1"] for r in recs),
    }


def conferir_dados(raiz):
    with open(os.path.join(BASE, "resultados", "medicoes.json")) as f:
        med = json.load(f)
    divergentes = []
    n = 0
    for a in med["arquivos"]:
        p = os.path.join(raiz, a["caminho"])
        if not os.path.exists(p):
            divergentes.append({"arquivo": a["nome"], "motivo": "ausente"})
            continue
        h = hashlib.sha256()
        with open(p, "rb") as f:
            while True:
                b = f.read(8 * MIB)
                if not b:
                    break
                h.update(b)
        n += 1
        if h.hexdigest() != a["sha256_arquivo"]:
            divergentes.append({"arquivo": a["nome"], "motivo": "sha256 diferente"})
    return {"arquivos_conferidos": n, "divergentes": divergentes, "ok": not divergentes}


def carregar_manifesto(raiz):
    with open(os.path.join(raiz, "manifesto.json")) as f:
        m = json.load(f)
    for a in m["arquivos"]:
        a.setdefault("tamanho_bytes", a.get("tamanho"))
    return m


def descansar(segundos, amostrador, intervalo=15):
    amostrador.iniciar()
    fim = time.perf_counter() + segundos
    recs = []
    while True:
        restante = fim - time.perf_counter()
        if restante <= 0:
            break
        time.sleep(min(intervalo, restante))
        recs.append(amostrador.amostrar())
    thr0 = amostrador.base_throttle
    thr1 = recs[-1]["throttle"] if recs else thr0
    return {
        "segundos": segundos,
        "throttle_incremento": {"core_count": thr1["core_count"] - thr0["core_count"], "package_count": (thr1["pkg_count"] - thr0["pkg_count"]) if thr1["pkg_count"] is not None else None},
        "temp_pkg_C": [r["temp_pkg_C"] for r in recs],
        "temp_core_max_C": [r["temp_core_max_C"] for r in recs],
        "bg": bg_resumo(recs),
        "freq_P_todos_MHz": [r["freq_P_todos_MHz"] for r in recs],
    }


def cmd_inventario(a):
    inv = {"data": time.strftime("%Y-%m-%d %H:%M:%S")}
    r = subprocess.run(["lscpu"], capture_output=True, text=True)
    inv["lscpu"] = r.stdout
    inv["kernel"] = os.uname().release
    caches = {}
    for rot, cpu in (("P", 0), ("E", 16)):
        lista = []
        for d in sorted(glob.glob("/sys/devices/system/cpu/cpu%d/cache/index*" % cpu)):
            lista.append(
                {
                    "nivel": ler(d + "/level"),
                    "tipo": ler(d + "/type"),
                    "tamanho": ler(d + "/size"),
                    "compartilhado_com": ler(d + "/shared_cpu_list"),
                    "vias": ler(d + "/ways_of_associativity"),
                    "linha_B": ler(d + "/coherency_line_size"),
                }
            )
        caches[rot + "_cpu%d" % cpu] = lista
    inv["caches_por_tipo"] = caches
    freq = {}
    for rot, lista in (("P", CPUS_P), ("E", CPUS_E)):
        g = {}
        for attr in ("cpuinfo_min_freq", "cpuinfo_max_freq", "scaling_min_freq", "scaling_max_freq", "base_frequency", "scaling_governor", "energy_performance_preference", "scaling_driver"):
            vals = sorted({ler("/sys/devices/system/cpu/cpu%d/cpufreq/%s" % (c, attr)) or "ilegivel" for c in lista})
            g[attr] = vals
        freq[rot] = g
    inv["frequencias_por_tipo"] = freq
    ps = {}
    for k in ("no_turbo", "status", "hwp_dynamic_boost", "min_perf_pct", "max_perf_pct", "turbo_pct", "num_pstates"):
        ps[k] = ler("/sys/devices/system/cpu/intel_pstate/" + k)
    inv["intel_pstate"] = ps
    rapl = {}
    for d in sorted(glob.glob("/sys/class/powercap/intel-rapl*:*")):
        if os.path.islink(d) or os.path.isdir(d):
            e = {"nome": ler(d + "/name"), "habilitado": ler(d + "/enabled"), "energy_uj_legivel": ler(d + "/energy_uj") is not None}
            for k in range(3):
                nome = ler("%s/constraint_%d_name" % (d, k))
                if nome:
                    e["constraint_%d" % k] = {
                        "nome": nome,
                        "limite_W": (ler_num("%s/constraint_%d_power_limit_uw" % (d, k)) or 0) / 1e6,
                        "janela_us": ler_num("%s/constraint_%d_time_window_us" % (d, k)),
                        "max_W": (ler_num("%s/constraint_%d_max_power_uw" % (d, k)) or 0) / 1e6,
                    }
            rapl[os.path.basename(d)] = e
    inv["rapl"] = rapl
    inv["ac_online"] = ler("/sys/class/power_supply/AC0/online")
    inv["bateria"] = {"status": ler("/sys/class/power_supply/BAT0/status"), "capacidade": ler("/sys/class/power_supply/BAT0/capacity")}
    try:
        inv["powerprofilesctl"] = subprocess.run(["powerprofilesctl", "get"], capture_output=True, text=True, timeout=5).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        inv["powerprofilesctl"] = "indisponivel"
    mem = ler_meminfo()
    inv["memoria_MiB"] = {k: mem[k] / 1024.0 for k in ("MemTotal", "MemAvailable", "MemFree", "Cached", "SwapTotal", "SwapFree", "Shmem")}
    inv["uptime_s"] = float(ler("/proc/uptime").split()[0])
    inv["loadavg"] = ler("/proc/loadavg")
    inv["disco"] = {
        "dispositivo": DISCO,
        "modelo": ler("/sys/block/%s/device/model" % DISCO),
        "scheduler": ler("/sys/block/%s/queue/scheduler" % DISCO),
        "nr_requests": ler("/sys/block/%s/queue/nr_requests" % DISCO),
        "df_home": subprocess.run(["df", "-B1", os.path.expanduser("~")], capture_output=True, text=True).stdout,
        "df_tmp": subprocess.run(["df", "-B1", "/tmp"], capture_output=True, text=True).stdout,
    }
    inv["versoes"] = {
        "python": sys.version.split()[0],
        "libzstd_python": SB.zstd.zstd_version,
        "zstd_cli": subprocess.run(["zstd", "--version"], capture_output=True, text=True).stdout.strip(),
        "gcc": subprocess.run(["gcc", "--version"], capture_output=True, text=True).stdout.splitlines()[0],
    }
    am = Amostrador(list(range(32)), os.getpid())
    am.iniciar()
    time.sleep(3)
    r0 = am.amostrar()
    inv["instantaneo_ocioso_3s"] = {k: r0[k] for k in ("temp_C", "temp_pkg_C", "temp_core_max_C", "temp_nvme_C", "freq_P_todos_MHz", "freq_E_todos_MHz", "bg_cores_proc", "bg_top", "loadavg1", "mem_disponivel_MiB", "swap_usado_MiB", "throttle")}
    inv["energia_rapl"] = "RAPL energy_uj legivel" if am.rapl else "RAPL ilegivel (energy_uj: permissao negada); limites de potencia (constraint_*_power_limit_uw) sim"
    inv["conferencia_dados"] = conferir_dados(a.dados) if os.path.isdir(a.dados) else {"ok": False, "motivo": "diretorio de dados ausente"}
    gravar_secao(a.saida, "inventario", inv)
    log("inventario gravado; dados ok=%s" % inv["conferencia_dados"].get("ok"))
    return inv


def montar_tar(raiz, destino):
    nomes = sorted(os.listdir(os.path.join(raiz, "silesia")))
    with tarfile.open(destino, "w", format=tarfile.GNU_FORMAT) as t:
        for n in nomes:
            p = os.path.join(raiz, "silesia", n)
            ti = tarfile.TarInfo(n)
            ti.size = os.path.getsize(p)
            ti.mode = 0o644
            ti.mtime = 0
            with open(p, "rb") as f:
                t.addfile(ti, f)
    return nomes


def parse_zstd_b(texto):
    res = {}
    ultimo = None
    for tok in re.split(r"[\r\n]+", texto):
        mf = re.search(r"(\d+)\s*->\s*(\d+)\s*\(x([\d.]+)\),\s*([\d.]+) MB/s,\s*([\d.]+) MB/s", tok)
        ml = re.match(r"^\s*(\d+)#", tok)
        if mf and ml:
            res[int(ml.group(1))] = mf.groups()
            ultimo = None
        elif mf:
            ultimo = mf.groups()
        elif ml and ultimo:
            res[int(ml.group(1))] = ultimo
            ultimo = None
    return {
        nivel: {"bytes_orig": int(g[0]), "bytes_comp": int(g[1]), "razao": float(g[2]), "comp_MBps": float(g[3]), "decomp_MBps": float(g[4])}
        for nivel, g in res.items()
    }


def cmd_calibracao(a):
    os.makedirs(a.trabalho, exist_ok=True)
    conf = conferir_dados(a.dados)
    tar = os.path.join(a.trabalho, "silesia.tar")
    nomes = montar_tar(a.dados, tar)
    h = hashlib.sha256()
    with open(tar, "rb") as f:
        while True:
            b = f.read(8 * MIB)
            if not b:
                break
            h.update(b)
    am = Amostrador([0, 16], os.getpid())
    execs = []
    for rep in range(a.repeticoes):
        for rot, cpu in (("P", 0), ("E", 16)):
            am.iniciar()
            t0 = time.time()
            r = subprocess.run(["taskset", "-c", str(cpu), "zstd", "-T1", "-b1", "-e3", "-i3", tar], capture_output=True)
            dur = time.time() - t0
            rec = am.amostrar()
            texto = (r.stdout + r.stderr).decode(errors="replace")
            niveis = parse_zstd_b(texto)
            execs.append(
                {
                    "tipo": rot,
                    "cpu": cpu,
                    "repeticao": rep,
                    "duracao_s": dur,
                    "niveis": niveis,
                    "bg_cores_durante": rec["bg_cores_proc"],
                    "bg_top": rec["bg_top"],
                    "temp_pkg_C_fim": rec["temp_pkg_C"],
                    "freq_ativa_MHz_fim": rec["freq_P_todos_MHz"] if rot == "P" else rec["freq_E_todos_MHz"],
                    "retorno": r.returncode,
                }
            )
            log("calibracao %s rep %d: %s" % (rot, rep, {k: (v["comp_MBps"], v["decomp_MBps"], v["razao"]) for k, v in niveis.items()}))
    res = {"tar": {"arquivos": nomes, "bytes": os.path.getsize(tar), "sha256": h.hexdigest(), "formato": "GNU tar via tarfile, mtime 0, modo 0644, sem uid/gid"}, "conferencia_dados": conf, "execucoes": execs, "por_tipo": {}}
    for rot in ("P", "E"):
        p = {}
        for nivel in ("1", "2", "3"):
            xs = [e["niveis"][int(nivel)] for e in execs if e["tipo"] == rot and int(nivel) in e["niveis"]]
            if not xs:
                continue
            p[nivel] = {
                "comp_MBps_mediana": mediana([x["comp_MBps"] for x in xs]),
                "comp_MBps_min": min(x["comp_MBps"] for x in xs),
                "comp_MBps_max": max(x["comp_MBps"] for x in xs),
                "decomp_MBps_mediana": mediana([x["decomp_MBps"] for x in xs]),
                "decomp_MBps_min": min(x["decomp_MBps"] for x in xs),
                "decomp_MBps_max": max(x["decomp_MBps"] for x in xs),
                "razao": xs[0]["razao"],
                "n": len(xs),
            }
        res["por_tipo"][rot] = p
    ref = REF_ZSTD1
    fat = {}
    for rot in ("P", "E"):
        l1 = res["por_tipo"][rot].get("1")
        if l1:
            fat[rot] = {
                "comp_fator": l1["comp_MBps_mediana"] / ref["comp_MBps"],
                "decomp_fator": l1["decomp_MBps_mediana"] / ref["decomp_MBps"],
                "razao_nossa_sobre_ref": l1["razao"] / ref["razao"],
            }
    if "P" in fat and "E" in fat:
        fat["E_sobre_P"] = {
            "comp_nivel1": res["por_tipo"]["E"]["1"]["comp_MBps_mediana"] / res["por_tipo"]["P"]["1"]["comp_MBps_mediana"],
            "decomp_nivel1": res["por_tipo"]["E"]["1"]["decomp_MBps_mediana"] / res["por_tipo"]["P"]["1"]["decomp_MBps_mediana"],
            "comp_nivel3": res["por_tipo"]["E"]["3"]["comp_MBps_mediana"] / res["por_tipo"]["P"]["3"]["comp_MBps_mediana"],
            "decomp_nivel3": res["por_tipo"]["E"]["3"]["decomp_MBps_mediana"] / res["por_tipo"]["P"]["3"]["decomp_MBps_mediana"],
        }
    res["fatores_vs_referencia_publica"] = fat
    res["referencia_publica"] = dict(ref, fonte="README do zstd 1.5.7, i7-9700K, lzbench, Silesia (silesia.tar), nivel 1 (valores informados no pedido)")
    res["limitacao"] = "base nao identica: silesia.tar montado aqui com tarfile (cabecalhos GNU e padding podem diferir do silesia.tar da referencia), CLI zstd 1.5.7 do sistema com -T1 e taskset, notebook com governor powersave"
    gravar_secao(a.saida, "calibracao", res)
    os.remove(tar)
    return res


def compilar_membw(destino):
    fonte = os.path.join(BASE, "membw.c")
    subprocess.run(["gcc", "-O3", "-fno-tree-loop-distribute-patterns", "-pthread", "-o", destino, fonte], check=True)
    return destino


def cmd_memoria(a):
    os.makedirs(a.trabalho, exist_ok=True)
    exe = compilar_membw(os.path.join(a.trabalho, "membw"))
    am = Amostrador(list(range(32)), os.getpid())
    mem = ler_meminfo()
    teto_mib = a.teto_gib * 1024
    if mem["MemAvailable"] / 1024.0 < teto_mib + 4096:
        teto_mib = max(1024, int(mem["MemAvailable"] / 1024.0 - 4096))
    confs = [
        ("1 P", [0]),
        ("2 P", [0, 2]),
        ("4 P", [0, 2, 4, 6]),
        ("8 P", list(range(0, 16, 2))),
        ("8 P + 8 E", CONFIGS["B"]),
        ("32 threads", list(range(32))),
    ]
    saida = []
    for rot, cpus in confs:
        n = len(cpus)
        vec = min(512, max(16, teto_mib // (3 * n)))
        am.iniciar()
        r = subprocess.run([exe, str(vec), str(a.mem_repeticoes), ",".join(str(c) for c in cpus), rot], capture_output=True, text=True)
        rec = am.amostrar()
        if r.returncode != 0:
            saida.append({"rotulo": rot, "erro": r.stderr})
            continue
        d = json.loads(r.stdout)
        d["bg_cores_durante"] = rec["bg_cores_proc"]
        d["bg_top"] = rec["bg_top"]
        d["temp_pkg_C_fim"] = rec["temp_pkg_C"]
        saida.append(d)
        log("memoria %s: copy %.1f triad %.1f GB/s (melhor), vetor %d MiB/thread" % (rot, d["kernels"]["copy"]["melhor_GBps"], d["kernels"]["triad"]["melhor_GBps"], vec))
    res = {
        "configuracoes": saida,
        "teto_ws_gib": teto_mib / 1024.0,
        "compilacao": "gcc -O3 -fno-tree-loop-distribute-patterns -pthread, sem -march=native",
        "bytes_contados": "estilo STREAM: copy 16 B/elemento, triad 24 B/elemento, sem contar write-allocate",
        "desvio": "vetores de 512 MiB por thread so ate 4 threads; com mais threads o tamanho cai (256, 128, 64 MiB) para manter o conjunto de trabalho total em ~6 GiB com a RAM da estacao em uso (3 vetores x 512 MiB x 32 threads = 48 GiB nao cabe nos 30 GB)",
    }
    gravar_secao(a.saida, "memoria", res)
    return res


def curva_interp(ts, cums, alvo):
    for k in range(1, len(ts)):
        if ts[k] >= alvo:
            f = (alvo - ts[k - 1]) / (ts[k] - ts[k - 1]) if ts[k] > ts[k - 1] else 1.0
            return cums[k - 1] + f * (cums[k] - cums[k - 1])
    return cums[-1]


def trabalhador_sust(i, cpu, modo, indices, raiz, mm, barreira, fila):
    gc.disable()
    os.sched_setaffinity(0, {cpu})
    dados = [SB.ler_fatia(raiz, c, MIB, off) for c, off in indices]
    comp, decomp = SB.fabricar("zstd-3", MIB)
    comprimidos = [comp(b) for b in dados]
    rec = [decomp(c) for c in comprimidos]
    exato = SB.sha256_blocos(rec) == SB.sha256_blocos(dados)
    del rec
    fila.put(("setup", i, exato, sum(len(c) for c in comprimidos), sum(len(b) for b in dados)))
    barreira.wait(timeout=600)
    n = 0
    off = 8 * i
    mono = time.monotonic
    deadline = struct.unpack_from("<d", mm, 2048)[0]
    k = 0
    if modo == "comp":
        lote = dados
    else:
        lote = comprimidos
    tam = [len(b) for b in dados]
    nb = len(lote)
    while True:
        if modo == "comp":
            comp(lote[k])
        else:
            decomp(lote[k])
        n += tam[k]
        struct.pack_into("<Q", mm, off, n)
        k += 1
        if k == nb:
            k = 0
        if mono() >= deadline:
            break
        if (k & 7) == 0:
            deadline = struct.unpack_from("<d", mm, 2048)[0]
    fila.put(("fim", i, n))


def condicao_parada(rec, base_swap):
    if rec["temp_pkg_C"] is not None and rec["temp_pkg_C"] >= 103:
        return "temperatura do pacote %.0f C" % rec["temp_pkg_C"]
    if rec["swap_usado_MiB"] - base_swap > 1024:
        return "swap crescendo (%.0f MiB)" % (rec["swap_usado_MiB"] - base_swap)
    if rec["mem_disponivel_MiB"] < 1000:
        return "memoria disponivel baixa (%.0f MiB)" % rec["mem_disponivel_MiB"]
    return None


def resumir_sustentado(rotulo, modo, cpus, duracao, curva, ts, cums, base, base_fim, setup, interrompido, finais, thr_base):
    jan_rajada = min(30.0, duracao / 10.0)
    jan_sust = min(300.0, duracao / 2.0)
    t_fim = ts[-1]
    cum_fim = cums[-1]
    rajada = curva_interp(ts, cums, jan_rajada) / jan_rajada / 1e6
    sust = (cum_fim - curva_interp(ts, cums, t_fim - jan_sust)) / jan_sust / 1e6 if t_fim > jan_sust else None
    vaz = [r["MBps"] for r in curva]
    por_minuto = []
    m = 60.0
    while m <= t_fim + 1e-6:
        por_minuto.append((curva_interp(ts, cums, m) - curva_interp(ts, cums, m - 60.0)) / 60.0 / 1e6)
        m += 60.0
    def janela(campo, ini, fim_):
        return media([r[campo] for r in curva if ini < r["t_s"] <= fim_])
    freq = {}
    for campo in ("freq_P_ativos_MHz", "freq_E_ativos_MHz", "freq_P_todos_MHz", "freq_E_todos_MHz"):
        freq[campo] = {"rajada": janela(campo, 0, jan_rajada), "sustentado": janela(campo, t_fim - jan_sust, t_fim), "geral": media([r[campo] for r in curva])}
    pk = [r["temp_pkg_C"] for r in curva if r["temp_pkg_C"] is not None]
    cm = [r["temp_core_max_C"] for r in curva if r["temp_core_max_C"] is not None]
    ult = curva[-1]["throttle"]
    thr = {
        "core_count_incremento": ult["core_count"] - thr_base["core_count"],
        "core_total_ms_incremento": ult["core_total_ms"] - thr_base["core_total_ms"],
        "package_count_incremento": (ult["pkg_count"] - thr_base["pkg_count"]) if ult["pkg_count"] is not None and thr_base["pkg_count"] is not None else None,
        "package_total_ms_incremento": (ult["pkg_total_ms"] - thr_base["pkg_total_ms"]) if ult["pkg_total_ms"] is not None and thr_base["pkg_total_ms"] is not None else None,
    }
    pot = [r["potencia_W"] for r in curva if r["potencia_W"] is not None]
    potencia = {"medida": bool(pot), "rajada_W": janela("potencia_W", 0, jan_rajada) if pot else None, "sustentado_W": janela("potencia_W", t_fim - jan_sust, t_fim) if pot else None} if pot else {"medida": False, "motivo": "RAPL ilegivel (energy_uj sem permissao)"}
    bgs = bg_resumo(curva)
    ruido = dict(bgs)
    try:
        import numpy as np

        if len(vaz) > 3 and np.std([r["bg_no_conjunto_cores"] for r in curva]) > 0:
            ruido["correlacao_bg_conjunto_vs_vazao"] = float(np.corrcoef([r["bg_no_conjunto_cores"] for r in curva], vaz)[0, 1])
    except ImportError:
        pass
    ruido["bg_inicio"] = {k: base[k] for k in ("bg_cores_proc", "bg_no_conjunto_cores", "bg_top")}
    ruido["bg_fim"] = {k: base_fim[k] for k in ("bg_cores_proc", "bg_no_conjunto_cores", "bg_top")}
    alvo_m = ALVO_MEDIA_MBPS
    res = {
        "rotulo": rotulo,
        "modo": modo,
        "cpus": cpus,
        "n_processos": len(cpus),
        "duracao_planejada_s": duracao,
        "duracao_real_s": t_fim,
        "janela_rajada_s": jan_rajada,
        "janela_sustentado_s": jan_sust,
        "rajada_MBps": rajada,
        "sustentado_MBps": sust,
        "derating_sustentado_sobre_rajada": (sust / rajada) if sust is not None else None,
        "min_MBps_janela_5s": min(vaz),
        "max_MBps_janela_5s": max(vaz),
        "media_total_MBps": cum_fim / t_fim / 1e6,
        "por_minuto_MBps": por_minuto,
        "PB_mes": {"rajada": pb_mes(rajada), "sustentado": pb_mes(sust) if sust is not None else None},
        "fracao_de_5PB_mes_media": {"rajada": rajada / alvo_m, "sustentado": (sust / alvo_m) if sust is not None else None},
        "fracao_de_20PB_mes_pico_x4": {"rajada": rajada / ALVO_PICO_MBPS, "sustentado": (sust / ALVO_PICO_MBPS) if sust is not None else None},
        "frequencia_MHz": freq,
        "temperatura": {"pkg_inicio_C": base["temp_pkg_C"], "pkg_max_C": max(pk) if pk else None, "pkg_fim_C": pk[-1] if pk else None, "core_max_C": max(cm) if cm else None, "pkg_media_sustentado_C": janela("temp_pkg_C", t_fim - jan_sust, t_fim), "zonas_inicio": base["temp_C"], "zonas_fim": curva[-1]["temp_C"]},
        "throttle": thr,
        "potencia": potencia,
        "ruido_de_fundo": ruido,
        "setup": setup,
        "interrompido": interrompido,
        "trabalhadores": finais,
        "curva": curva,
    }
    return res


def sustentado_um(a, rotulo, modo, cpus, duracao, intervalo, manifesto):
    os.sched_setaffinity(0, {CPU_PAI})
    am = Amostrador(cpus, os.getpid())
    am.iniciar()
    time.sleep(3)
    base = am.amostrar()
    mem0 = ler_meminfo()
    base_swap = (mem0["SwapTotal"] - mem0["SwapFree"]) / 1024.0
    log("%s (%s, N=%d): temperatura inicial %s C, bg %.2f cores" % (rotulo, modo, len(cpus), base["temp_pkg_C"], base["bg_cores_proc"]))
    pool = SB.pool_blocos(manifesto)
    n = len(cpus)
    ctx = mp.get_context("fork")
    mm = mmap.mmap(-1, 4096)
    fila = ctx.Queue()
    barreira = ctx.Barrier(n + 1)
    raiz = os.path.abspath(a.dados)
    procs = [ctx.Process(target=trabalhador_sust, args=(i, cpus[i], modo, pool[i::n], raiz, mm, barreira, fila)) for i in range(n)]
    for p in procs:
        p.start()
    setup = {"bit_exato": True, "bytes_comp": 0, "bytes_orig": 0}
    for _ in range(n):
        m = fila.get(timeout=600)
        setup["bit_exato"] = setup["bit_exato"] and m[2]
        setup["bytes_comp"] += m[3]
        setup["bytes_orig"] += m[4]
    setup["razao_zstd3_no_conjunto_do_teste"] = setup["bytes_orig"] / setup["bytes_comp"]
    setup["blocos_por_processo"] = len(pool) // n
    am.iniciar()
    thr_base = am.base_throttle
    deadline = time.monotonic() + duracao + 0.5
    struct.pack_into("<d", mm, 2048, deadline)
    barreira.wait(timeout=600)
    t0 = time.perf_counter()
    curva = []
    ts = [0.0]
    cums = [0.0]
    interrompido = None
    k = 0
    while True:
        k += 1
        alvo = t0 + k * intervalo
        if alvo - t0 > duracao + 1e-6:
            break
        espera = alvo - time.perf_counter()
        if espera > 0:
            time.sleep(espera)
        rec = am.amostrar()
        cum = sum(struct.unpack_from("<Q", mm, 8 * i)[0] for i in range(n))
        t = time.perf_counter() - t0
        rec["MBps"] = (cum - cums[-1]) / (t - ts[-1]) / 1e6
        rec["cum_MB"] = cum / 1e6
        rec["t_s"] = t
        curva.append(rec)
        ts.append(t)
        cums.append(float(cum))
        motivo = condicao_parada(rec, base_swap)
        if motivo:
            interrompido = {"em_s": t, "motivo": motivo}
            struct.pack_into("<d", mm, 2048, time.monotonic() - 1.0)
            log("%s interrompido: %s" % (rotulo, motivo))
            break
        if k % 12 == 0:
            log("%s t=%.0fs %.0f MB/s pkg %.0f C P %.0f MHz bg %.2f" % (rotulo, t, rec["MBps"], rec["temp_pkg_C"] or 0, rec["freq_P_ativos_MHz"] or 0, rec["bg_no_conjunto_cores"]))
    finais = []
    for _ in range(n):
        m = fila.get(timeout=120)
        finais.append({"i": m[1], "bytes": m[2]})
    for p in procs:
        p.join(timeout=60)
    t_total = time.perf_counter() - t0
    finais.sort(key=lambda x: x["i"])
    for f in finais:
        f["MBps_medio"] = f["bytes"] / t_total / 1e6
    bf = Amostrador(cpus, os.getpid())
    bf.iniciar()
    time.sleep(3)
    base_fim = bf.amostrar()
    res = resumir_sustentado(rotulo, modo, cpus, duracao, curva, ts, cums, base, base_fim, setup, interrompido, finais, thr_base)
    log("%s: rajada %.0f MB/s, sustentado %s MB/s, derating %s" % (rotulo, res["rajada_MBps"], res["sustentado_MBps"], res["derating_sustentado_sobre_rajada"]))
    return res


def cmd_sustentado(a):
    manifesto = carregar_manifesto(a.dados)
    conf = conferir_dados(a.dados)
    if not conf["ok"]:
        log("AVISO: conferencia de dados falhou: %s" % conf["divergentes"])
    gravar_secao(a.saida, "sustentado_meta", {"conferencia_dados": conf, "duracao_s": a.duracao, "intervalo_s": a.intervalo, "descanso_s": a.descanso, "cpu_do_processo_pai": CPU_PAI})
    plano = [x.strip() for x in a.configs.split(",")]
    descansos = []
    am_r = Amostrador(list(range(32)), os.getpid())
    for idx, item in enumerate(plano):
        espera = a.descanso_antes_a if idx == 0 else a.descanso
        if espera > 0 and (idx > 0 or a.descanso_inicial):
            log("descanso de %d s" % espera)
            descansos.append(dict(descansar(espera, am_r), antes_de=item))
            gravar_secao(a.saida, "sustentado_descansos", descansos)
        cfg, modo = (item.split("_") + ["comp"])[:2]
        modo = "comp" if modo.startswith("comp") else "decomp"
        cpus = CONFIGS[cfg]
        res = sustentado_um(a, item, modo, cpus, a.duracao, a.intervalo, manifesto)
        gravar_subsecao(a.saida, "sustentado", item, res)


def modo_io_comp_worker(i, cpu, p, mm, barreira, fila):
    gc.disable()
    os.sched_setaffinity(0, {cpu})
    comp, decomp = SB.fabricar("zstd-3", MIB)
    modo = p["modo"]
    fd_in = None
    escritor = None
    leitor = None
    fds_saida = None
    buf_in = mmap.mmap(-1, MIB)
    lat_bloco = array.array("d")
    lat_cpu = array.array("d")
    lat_leit = array.array("d")
    if modo in ("comp", "teto-leitura", "preparar", "comp-ram"):
        fd_in = os.open(p["entrada"], os.O_RDONLY | os.O_DIRECT)
    if modo in ("comp", "decomp-grava", "teto-escrita", "preparar"):
        flags = os.O_WRONLY | os.O_CREAT | os.O_TRUNC
        if p["saida_odirect"]:
            flags |= os.O_DIRECT
        fds_saida = os.open(p["saida"], flags, 0o600)
        escritor = Escritor(fds_saida, p["cap"], p["saida_odirect"])
    if modo in ("decomp", "decomp-grava"):
        fds = [os.open(f, os.O_RDONLY | os.O_DIRECT) for f in p["arquivos_comp"]]
        leitor = Leitor(fds, [os.fstat(f).st_size for f in fds])
    ini, fim = p["regiao"]
    pos = ini
    blocos_ram = []
    if modo == "comp-ram":
        for j in range(48):
            os.preadv(fd_in, [buf_in], ini + j * MIB)
            blocos_ram.append(bytes(buf_in))
    if modo == "teto-escrita":
        os.preadv(os.open(p["entrada"], os.O_RDONLY | os.O_DIRECT), [buf_in], ini)
    bytes_in = 0
    bytes_comp = 0
    cnt = 0
    fila.put(("pronto", i))
    barreira.wait(timeout=600)
    deadline = struct.unpack_from("<d", mm, 2048)[0] if modo != "preparar" else float("inf")
    t_ini = time.perf_counter()
    c_ini = time.process_time()
    mono = time.monotonic
    pc = time.perf_counter
    while True:
        t0 = pc()
        if modo == "comp" or modo == "preparar":
            tr = pc()
            os.preadv(fd_in, [buf_in], pos)
            lat_leit.append(pc() - tr)
            tc = pc()
            c = comp(buf_in)
            lat_cpu.append(pc() - tc)
            fr = struct.pack("<II", MIB, len(c))
            escritor.escrever(fr)
            escritor.escrever(c)
            bytes_comp += len(c) + 8
            bytes_in += MIB
            pos += MIB
            if pos >= fim:
                if modo == "preparar":
                    lat_bloco.append(pc() - t0)
                    break
                pos = ini
        elif modo == "comp-ram":
            tc = pc()
            c = comp(blocos_ram[cnt % 48])
            lat_cpu.append(pc() - tc)
            bytes_comp += len(c) + 8
            bytes_in += MIB
        elif modo == "teto-leitura":
            tr = pc()
            os.preadv(fd_in, [buf_in], pos)
            lat_leit.append(pc() - tr)
            bytes_in += MIB
            pos += MIB
            if pos >= fim:
                pos = ini
        elif modo == "teto-escrita":
            escritor.escrever(buf_in)
            bytes_in += MIB
        else:
            fr = leitor.proximo()
            tc = pc()
            out = decomp(fr)
            lat_cpu.append(pc() - tc)
            bytes_in += len(out)
            if escritor is not None:
                escritor.escrever(out)
        lat_bloco.append(pc() - t0)
        cnt += 1
        struct.pack_into("<Q", mm, 8 * i, bytes_in)
        struct.pack_into("<Q", mm, 512 + 8 * i, (leitor.lidos if leitor else (cnt * MIB if fd_in is not None else 0)))
        struct.pack_into("<Q", mm, 1024 + 8 * i, escritor.escritos if escritor else 0)
        if modo != "preparar" and (cnt & 3) == 0:
            if mono() >= deadline:
                break
            deadline = struct.unpack_from("<d", mm, 2048)[0]
    decorrido = pc() - t_ini
    cpu_usada = time.process_time() - c_ini
    flush_t = 0.0
    if escritor is not None:
        tf = pc()
        escritor.fechar(modo == "preparar")
        flush_t = pc() - tf
    status = ler("/proc/self/status") or ""
    def kb(campo):
        m = re.search(campo + r":\s+(\d+) kB", status)
        return int(m.group(1)) if m else None
    lat_leit_all = lat_leit
    if leitor is not None:
        lat_leit_all = leitor.lat
    fila.put(
        (
            "fim",
            i,
            {
                "bytes_in": bytes_in,
                "bytes_comp": bytes_comp,
                "decorrido_s": decorrido,
                "cpu_frac": cpu_usada / decorrido if decorrido > 0 else None,
                "vmhwm_kB": kb("VmHWM"),
                "vmrss_kB": kb("VmRSS"),
                "flush_final_s": flush_t,
                "bytes_leitos_disco": leitor.lidos if leitor else None,
                "bytes_escritos_disco": escritor.escritos if escritor else 0,
                "lat_bloco": lat_bloco.tobytes(),
                "lat_cpu": lat_cpu.tobytes(),
                "lat_leitura": lat_leit_all.tobytes(),
                "lat_escrita": escritor.lat.tobytes() if escritor else b"",
            },
        )
    )


class Escritor:
    def __init__(self, fd, cap, odireto):
        self.fd = fd
        self.cap = cap
        self.odireto = odireto
        self.buf = mmap.mmap(-1, MIB)
        self.mv = memoryview(self.buf)
        self.fill = 0
        self.off = 0
        self.escritos = 0
        self.lat = array.array("d")
        self.desde_sync = 0

    def escrever(self, dados):
        mv = memoryview(dados)
        n = len(mv)
        pos = 0
        while pos < n:
            k = min(MIB - self.fill, n - pos)
            self.mv[self.fill : self.fill + k] = mv[pos : pos + k]
            self.fill += k
            pos += k
            if self.fill == MIB:
                self.descarregar()

    def descarregar(self):
        t = time.perf_counter()
        os.pwrite(self.fd, self.buf, self.off)
        self.off += MIB
        self.escritos += MIB
        self.fill = 0
        if self.off >= self.cap:
            self.off = 0
        if not self.odireto:
            self.desde_sync += MIB
            if self.desde_sync >= 128 * MIB:
                os.posix_fadvise(self.fd, 0, 0, os.POSIX_FADV_DONTNEED)
                self.desde_sync = 0
        self.lat.append(time.perf_counter() - t)

    def fechar(self, completar):
        if completar and self.fill:
            self.mv[self.fill :] = bytes(MIB - self.fill)
            self.fill = MIB
            self.descarregar()
        if not self.odireto:
            os.fdatasync(self.fd)
        self.mv.release()
        os.close(self.fd)


class Leitor:
    def __init__(self, fds, tamanhos):
        self.fds = fds
        self.tam = tamanhos
        self.f = 0
        self.off = 0
        self.pend = bytearray()
        self.p = 0
        self.buf = mmap.mmap(-1, MIB)
        self.mv = memoryview(self.buf)
        self.lat = array.array("d")
        self.lidos = 0
        self.fim_arq = False

    def proximo_arquivo(self):
        self.f = (self.f + 1) % len(self.fds)
        self.off = 0
        self.fim_arq = False
        self.pend = bytearray()
        self.p = 0

    def ler_bloco(self):
        t = time.perf_counter()
        n = os.preadv(self.fds[self.f], [self.buf], self.off)
        self.lat.append(time.perf_counter() - t)
        if self.p:
            del self.pend[: self.p]
            self.p = 0
        self.pend += self.mv[:n]
        self.off += n
        self.lidos += n
        if n == 0 or self.off >= self.tam[self.f]:
            self.fim_arq = True

    def proximo(self):
        while True:
            disp = len(self.pend) - self.p
            if disp >= 8:
                olen, clen = struct.unpack_from("<II", self.pend, self.p)
                if olen == 0:
                    self.proximo_arquivo()
                    continue
                if disp >= 8 + clen:
                    fr = self.pend[self.p + 8 : self.p + 8 + clen]
                    self.p += 8 + clen
                    return fr
            if self.fim_arq:
                self.proximo_arquivo()
                continue
            self.ler_bloco()


def gerar_entrada(destino, gib, semente, raiz, manifesto):
    import numpy as np

    reais = [x for x in manifesto["arquivos"] if not x["sintetico"]]
    dados = []
    for x in reais:
        with open(os.path.join(raiz, x["caminho"]), "rb") as f:
            dados.append(np.frombuffer(f.read(), dtype=np.uint8))
    fatias = [min(len(d), 8 * MIB) for d in dados]
    pesos = np.array(fatias, dtype=np.float64)
    pesos /= pesos.sum()
    rng = np.random.default_rng(semente)
    nb = int(gib * 1024)
    ks = rng.choice(len(dados), size=nb, p=pesos)
    buf = mmap.mmap(-1, MIB)
    arr = np.frombuffer(buf, dtype=np.uint8)
    fd = os.open(destino, os.O_WRONLY | os.O_CREAT | os.O_TRUNC | os.O_DIRECT, 0o600)
    comp, _ = SB.fabricar("zstd-3", MIB)
    amostra_in = 0
    amostra_out = 0
    t0 = time.time()
    for b in range(nb):
        k = int(ks[b])
        off = int(rng.integers(0, fatias[k] - MIB + 1))
        arr[:] = dados[k][off : off + MIB]
        pos = rng.integers(0, MIB, size=16)
        arr[pos] = rng.integers(0, 256, size=16, dtype=np.uint8)
        if b % 16 == 0:
            amostra_out += len(comp(buf))
            amostra_in += MIB
        os.write(fd, buf)
    os.fsync(fd)
    os.close(fd)
    pesos_arq = {x["nome"]: float(p) for x, p in zip(reais, pesos)}
    return {
        "bytes": nb * MIB,
        "semente": semente,
        "arquivos_fonte": [x["nome"] for x in reais],
        "pesos_por_arquivo": pesos_arq,
        "mutacao": "16 bytes aleatorios por bloco de 1 MiB, posicoes e valores do mesmo gerador",
        "offsets": "bloco de 1 MiB lido de offset aleatorio (sem alinhamento) dentro dos 8 MiB iniciais de um arquivo real sorteado com peso proporcional ao tamanho da fatia de 8 MiB (mesma definicao de mix_real de medicoes.json)",
        "razao_zstd3_amostra_1_em_16": amostra_in / amostra_out,
        "blocos_amostrados": amostra_in // MIB,
        "tempo_geracao_s": time.time() - t0,
    }


def agregar_lat(resultados, campo):
    import numpy as np

    partes = [np.frombuffer(r[campo], dtype=np.float64) for r in resultados if len(r[campo])]
    if not partes:
        return None
    x = np.concatenate(partes) * 1e3
    return {"n": int(x.size), "p50_ms": float(np.percentile(x, 50)), "p99_ms": float(np.percentile(x, 99)), "p999_ms": float(np.percentile(x, 99.9)), "max_ms": float(x.max()), "media_ms": float(x.mean())}


def executar_io(a, rotulo, modo, cpus, params_base, duracao, intervalo):
    os.sched_setaffinity(0, {CPU_PAI})
    n = len(cpus)
    am = Amostrador(cpus, os.getpid())
    am.iniciar()
    time.sleep(3)
    base = am.amostrar()
    mem0 = ler_meminfo()
    base_swap = (mem0["SwapTotal"] - mem0["SwapFree"]) / 1024.0
    ctx = mp.get_context("fork")
    mm = mmap.mmap(-1, 4096)
    fila = ctx.Queue()
    barreira = ctx.Barrier(n + 1)
    procs = []
    for i in range(n):
        p = params_base(i, n)
        p["modo"] = modo
        procs.append(ctx.Process(target=modo_io_comp_worker, args=(i, cpus[i], p, mm, barreira, fila)))
    for pr in procs:
        pr.start()
    for _ in range(n):
        fila.get(timeout=600)
    am.iniciar()
    thr_base = am.base_throttle
    struct.pack_into("<d", mm, 2048, time.monotonic() + duracao + 0.5)
    barreira.wait(timeout=600)
    t0 = time.perf_counter()
    curva = []
    ts = [0.0]
    cums = [0.0]
    interrompido = None
    k = 0
    while True:
        k += 1
        alvo = t0 + k * intervalo
        if alvo - t0 > duracao + 1e-6:
            break
        espera = alvo - time.perf_counter()
        if espera > 0:
            time.sleep(espera)
        rec = am.amostrar()
        cum = sum(struct.unpack_from("<Q", mm, 8 * i)[0] for i in range(n))
        t = time.perf_counter() - t0
        rec["MBps"] = (cum - cums[-1]) / (t - ts[-1]) / 1e6
        rec["t_s"] = t
        curva.append(rec)
        ts.append(t)
        cums.append(float(cum))
        motivo = condicao_parada(rec, base_swap)
        if motivo:
            interrompido = {"em_s": t, "motivo": motivo}
            struct.pack_into("<d", mm, 2048, time.monotonic() - 1.0)
            break
    resultados = []
    for _ in range(n):
        m = fila.get(timeout=300)
        resultados.append(m[2])
    for pr in procs:
        pr.join(timeout=120)
    t_total = time.perf_counter() - t0
    bf = Amostrador(cpus, os.getpid())
    bf.iniciar()
    time.sleep(3)
    base_fim = bf.amostrar()
    skip = min(10.0, duracao / 6.0)
    mbps_est = (cums[-1] - curva_interp(ts, cums, skip)) / (ts[-1] - skip) / 1e6
    disco_l = media([r["disco"]["leitura_MBps"] for r in curva if r["disco"] and r["t_s"] > skip])
    disco_e = media([r["disco"]["escrita_MBps"] for r in curva if r["disco"] and r["t_s"] > skip])
    util = media([r["disco"]["util_pct"] for r in curva if r["disco"] and r["t_s"] > skip])
    fila_m = media([r["disco"]["fila_media"] for r in curva if r["disco"] and r["t_s"] > skip])
    metade = ts[-1] / 2.0
    ult_vaz = (cums[-1] - curva_interp(ts, cums, metade)) / (ts[-1] - metade) / 1e6
    ult_l = media([r["disco"]["leitura_MBps"] for r in curva if r["disco"] and r["t_s"] > metade])
    ult_e = media([r["disco"]["escrita_MBps"] for r in curva if r["disco"] and r["t_s"] > metade])
    primeira_e = media([r["disco"]["escrita_MBps"] for r in curva if r["disco"] and r["t_s"] <= min(30.0, ts[-1] / 4.0)])
    flush_max = max(r["flush_final_s"] for r in resultados)
    bytes_comp = sum(r["bytes_comp"] for r in resultados)
    bytes_in = sum(r["bytes_in"] for r in resultados)
    cpu_frac = media([r["cpu_frac"] for r in resultados])
    res = {
        "rotulo": rotulo,
        "modo": modo,
        "n_processos": n,
        "cpus": cpus,
        "duracao_planejada_s": duracao,
        "duracao_real_s": t_total,
        "vazao_entrada_MBps_apos_aquecimento": mbps_est,
        "vazao_entrada_MBps_total": cums[-1] / ts[-1] / 1e6,
        "vazao_entrada_MBps_incluindo_flush_final": cums[-1] / (ts[-1] + flush_max) / 1e6,
        "vazao_entrada_MBps_segunda_metade": ult_vaz,
        "disco_leitura_MBps_segunda_metade": ult_l,
        "disco_escrita_MBps_segunda_metade": ult_e,
        "disco_escrita_MBps_primeiros_segundos": primeira_e,
        "min_MBps_janela": min(r["MBps"] for r in curva),
        "max_MBps_janela": max(r["MBps"] for r in curva),
        "PB_mes": pb_mes(mbps_est),
        "fracao_de_5PB_mes_media": mbps_est / ALVO_MEDIA_MBPS,
        "fracao_de_20PB_mes_pico_x4": mbps_est / ALVO_PICO_MBPS,
        "disco_leitura_MBps": disco_l,
        "disco_escrita_MBps": disco_e,
        "disco_util_pct": util,
        "disco_fila_media": fila_m,
        "cpu_frac_media_por_processo": cpu_frac,
        "cpu_frac_min": min(r["cpu_frac"] for r in resultados),
        "rss_max_MiB": max(r["vmhwm_kB"] for r in resultados) / 1024.0,
        "rss_fim_MiB_media": media([r["vmrss_kB"] for r in resultados]) / 1024.0,
        "razao_obtida": (bytes_in / bytes_comp) if bytes_comp else None,
        "flush_final_s_max": flush_max,
        "latencia": {c: agregar_lat(resultados, c) for c in ("lat_bloco", "lat_cpu", "lat_leitura", "lat_escrita")},
        "freq_P_ativos_MHz": media([r["freq_P_ativos_MHz"] for r in curva]),
        "freq_E_ativos_MHz": media([r["freq_E_ativos_MHz"] for r in curva]),
        "temp_pkg_inicio_C": base["temp_pkg_C"],
        "temp_pkg_max_C": max(r["temp_pkg_C"] for r in curva if r["temp_pkg_C"] is not None),
        "temp_nvme_max_C": max([r["temp_nvme_C"] for r in curva if r["temp_nvme_C"] is not None] or [None]),
        "ruido_de_fundo": dict(bg_resumo(curva), bg_inicio=base["bg_cores_proc"], bg_fim=base_fim["bg_cores_proc"]),
        "throttle_incremento": {
            "core_count": curva[-1]["throttle"]["core_count"] - thr_base["core_count"],
            "package_count": (curva[-1]["throttle"]["pkg_count"] - thr_base["pkg_count"]) if curva[-1]["throttle"]["pkg_count"] is not None else None,
        },
        "interrompido": interrompido,
        "curva": [{k: v for k, v in r.items() if k in ("t_s", "MBps", "disco", "temp_pkg_C", "freq_P_ativos_MHz", "freq_E_ativos_MHz", "bg_no_conjunto_cores", "bg_top")} for r in curva],
    }
    log("%s: %.0f MB/s, disco R %.0f W %.0f MB/s, util %.0f%%, cpu_frac %.2f" % (rotulo, mbps_est, disco_l or 0, disco_e or 0, util or 0, cpu_frac or 0))
    return res


def classificar_limite(res, teto_leit, teto_escr):
    if res["modo"] == "comp-ram":
        return "CPU (controle sem I/O)"
    cf = res["cpu_frac_media_por_processo"] or 0
    r = res["disco_leitura_MBps_segunda_metade"] or 0
    w = res["disco_escrita_MBps_segunda_metade"] or 0
    fr = max(r / teto_leit if teto_leit else 0, w / teto_escr if teto_escr else 0)
    util = res["disco_util_pct"] or 0
    if cf >= 0.90 and fr < 0.80 and util < 85:
        return "CPU (cpu_frac %.2f, disco a %.0f%% do teto sustentado, util %.0f%%)" % (cf, 100 * fr, util)
    if cf < 0.85 and (fr >= 0.80 or util >= 85):
        return "disco (cpu_frac %.2f, disco a %.0f%% do teto sustentado, util %.0f%%)" % (cf, 100 * fr, util)
    return "misto/indefinido (cpu_frac %.2f, disco a %.0f%% do teto sustentado, util %.0f%%)" % (cf, 100 * fr, util)


def cmd_io_disco(a):
    manifesto = carregar_manifesto(a.dados)
    conf = conferir_dados(a.dados)
    d = a.io_dir
    os.makedirs(d, mode=0o700, exist_ok=True)
    livre = shutil.disk_usage(d).free
    resultado = {"diretorio": d, "conferencia_dados": conf, "livre_inicial_GiB": livre / GIB}
    if livre < 70 * GIB:
        resultado["erro"] = "espaco livre insuficiente"
        gravar_secao(a.saida, "io_disco", resultado)
        return resultado
    ds0 = ler_diskstats(DISCO)
    entrada = os.path.join(d, "entrada.bin")
    try:
        os.sched_setaffinity(0, {CPU_PAI})
        log("gerando entrada de %.1f GiB" % a.io_gib)
        resultado["entrada"] = gerar_entrada(entrada, a.io_gib, 20261104, os.path.abspath(a.dados), manifesto)
        log("entrada pronta: razao (amostra) %.3f em %.0f s" % (resultado["entrada"]["razao_zstd3_amostra_1_em_16"], resultado["entrada"]["tempo_geracao_s"]))
        nblocos = int(a.io_gib * 1024)
        cap = a.io_cap_mib * MIB

        def regiao(i, n):
            return (i * nblocos // n * MIB, (i + 1) * nblocos // n * MIB)

        def p_preparar(i, n):
            return {"entrada": entrada, "regiao": regiao(i, n), "saida": os.path.join(d, "comp_%02d.bin" % i), "saida_odirect": True, "cap": 1 << 50}

        cpus16 = CONFIGS["B"]
        t0 = time.time()
        os.sched_setaffinity(0, {CPU_PAI})
        ctx = mp.get_context("fork")
        mm = mmap.mmap(-1, 4096)
        fila = ctx.Queue()
        barreira = ctx.Barrier(16 + 1)
        procs = []
        for i in range(16):
            p = p_preparar(i, 16)
            p["modo"] = "preparar"
            procs.append(ctx.Process(target=modo_io_comp_worker, args=(i, cpus16[i], p, mm, barreira, fila)))
        for pr in procs:
            pr.start()
        for _ in range(16):
            fila.get(timeout=600)
        barreira.wait(timeout=600)
        rs = []
        for _ in range(16):
            rs.append(fila.get(timeout=900)[2])
        for pr in procs:
            pr.join(timeout=60)
        bc = sum(r["bytes_comp"] for r in rs)
        bi = sum(r["bytes_in"] for r in rs)
        resultado["preparo_comprimido"] = {
            "razao_exata_zstd3_blocos_1MiB_com_cabecalho_8B": bi / bc,
            "bytes_comprimidos": bc,
            "arquivos": 16,
            "duracao_s": time.time() - t0,
            "vazao_total_MBps": bi / 1e6 / (time.time() - t0),
        }
        log("comprimido preparado: razao exata %.3f em %.0f s" % (resultado["preparo_comprimido"]["razao_exata_zstd3_blocos_1MiB_com_cabecalho_8B"], resultado["preparo_comprimido"]["duracao_s"]))
        gravar_secao(a.saida, "io_disco", resultado)

        def p_saida(odirect):
            def f(i, n):
                return {"entrada": entrada, "regiao": regiao(i, n), "saida": os.path.join(d, "saida_%02d.bin" % i), "saida_odirect": odirect, "cap": cap}
            return f

        def p_decomp(i, n):
            return {"arquivos_comp": [os.path.join(d, "comp_%02d.bin" % j) for j in range(i, 16, n)], "regiao": (0, 0), "saida": os.path.join(d, "saida_%02d.bin" % i), "saida_odirect": True, "cap": cap, "entrada": entrada}

        cpus8 = CONFIGS["A"]
        descanso = []
        am_r = Amostrador(list(range(32)), os.getpid())
        tetos = {}
        for rot, modo, cpus, builder, dur in (
            ("teto_leitura_N16", "teto-leitura", cpus16, p_saida(True), a.io_teto_leitura),
            ("teto_escrita_N16", "teto-escrita", cpus16, p_saida(True), a.io_teto_escrita),
        ):
            r = executar_io(a, rot, modo, cpus, builder, dur, 3.0)
            tetos[rot] = r
        resultado["tetos_disco"] = tetos
        gravar_secao(a.saida, "io_disco", resultado)
        teto_l = tetos["teto_leitura_N16"]["disco_leitura_MBps"] or 0
        teto_e = tetos["teto_escrita_N16"]["disco_escrita_MBps_segunda_metade"] or 0
        resultado["teto_leitura_MBps"] = teto_l
        resultado["teto_escrita_sustentado_MBps_segunda_metade"] = teto_e
        plano = []
        for item in a.io_plano.split(","):
            modo, n, var, dur = item.split(":")
            plano.append((modo, int(n), var, float(dur)))
        resultado["testes"] = {}
        for modo, n, var, dur in plano:
            if a.io_descanso > 0:
                descanso.append(descansar(a.io_descanso, am_r))
            cpus = cpus8 if n == 8 else cpus16
            if modo == "comp":
                rot = "comp_N%d_%s" % (n, var)
                builder = p_saida(var == "odirect")
                modo_w = "comp"
            elif modo == "ram":
                rot = "controle_cpu_sem_io_N%d" % n
                builder = p_saida(True)
                modo_w = "comp-ram"
            elif modo == "decomp":
                rot = "decomp_N%d_%s" % (n, var)
                builder = p_decomp
                modo_w = "decomp-grava" if var == "grava" else "decomp"
            r = executar_io(a, rot, modo_w, cpus, builder, dur, a.intervalo)
            r["variante_saida"] = {"odirect": "escrita O_DIRECT em blocos de 1 MiB alinhados", "bufferizado": "escrita bufferizada (sem O_DIRECT) em blocos de 1 MiB, sem fsync periodico, fadvise DONTNEED a cada 128 MiB e fdatasync no fim (tempo do flush final em flush_final_s_max)", "descartar": "descompressao sem escrita (saida descartada)", "grava": "descompressao gravando o bloco de 1 MiB de volta no NVMe com O_DIRECT", "cpu": "controle: compressao em memoria de 48 blocos do mesmo arquivo de entrada, sem nenhum I/O de disco"}[var]
            r["limitante"] = classificar_limite(r, teto_l, teto_e)
            resultado["testes"][rot] = r
            gravar_secao(a.saida, "io_disco", dict(resultado, descansos=descanso))
        ds1 = ler_diskstats(DISCO)
        resultado["escrita_fisica_total_GiB"] = (ds1["wr_setores"] - ds0["wr_setores"]) * 512 / GIB
        resultado["leitura_fisica_total_GiB"] = (ds1["rd_setores"] - ds0["rd_setores"]) * 512 / GIB
        resultado["pegada_maxima_em_disco_GiB"] = a.io_gib + resultado["preparo_comprimido"]["bytes_comprimidos"] / GIB + 16 * a.io_cap_mib / 1024.0
        resultado["descansos"] = descanso
    finally:
        shutil.rmtree(d, ignore_errors=True)
        resultado["limpeza"] = {"diretorio_removido": not os.path.exists(d)}
        gravar_secao(a.saida, "io_disco", resultado)
    return resultado


def fmt(x, c=0):
    if x is None:
        return "-"
    return ("{:,.%df}" % c).format(x).replace(",", "X").replace(".", ",").replace("X", ".")


def fmtv(x, c=2):
    if x is None:
        return "-"
    return (("%." + str(c) + "f") % x).replace(".", ",")


def cmd_resumo(a):
    dados = carregar_json(a.saida)
    linhas = []
    w = linhas.append
    inv = dados.get("inventario", {})
    cal = dados.get("calibracao", {})
    mem = dados.get("memoria", {})
    sus = dados.get("sustentado", {})
    io = dados.get("io_disco", {})
    w("# capacidade local: quanto de 5 PB/mes esta maquina sustenta (i9-14900HX)")
    w("")
    w("Gerado por `bench/capacidade_local.py resumo` a partir de `resultados/capacidade_local.json`. Alvos: 5 PB/mes = %s MB/s medios (30 dias = 2,592e6 s); pico x4 = %s MB/s (20 PB/mes sustentado). PB/mes = MB/s x 2,592e6 / 1e9. Toda a CPU e zstd-3 em blocos independentes de 1 MiB do mix real (15 arquivos Silesia + Canterbury Large, sem sinteticos; ver LIMITACOES no README)." % (fmt(ALVO_MEDIA_MBPS), fmt(ALVO_PICO_MBPS)))
    w("")
    w("## 1. Tabela unica")
    w("")
    w("| teste | MB/s sustentado | MB/s rajada | derating | PB/mes (sust.) | % de 5 PB/mes | % do pico x4 (20 PB/mes) | limitante | observacao |")
    w("|---|---|---|---|---|---|---|---|---|")
    linhas_json = []
    ordem = ["A_comp", "B_comp", "C_comp", "B_decomp"]
    nomes = {"A_comp": "A: 8 P fisicos, compressao", "B_comp": "B: 8P+8E (16), compressao", "C_comp": "C: 32 threads, compressao", "B_decomp": "B: 8P+8E (16), descompressao"}
    for k in ordem:
        s = sus.get(k)
        if not s:
            continue
        sm = s["sustentado_MBps"]
        obs = "interrompido: %s" % s["interrompido"]["motivo"] if s.get("interrompido") else "temp. pkg max %s C; ruido de fundo medio %.2f core" % (fmtv(s["temperatura"]["pkg_max_C"], 0), s["ruido_de_fundo"]["bg_no_conjunto_cores_medio"] or 0)
        lim = "CPU (termica/potencia de notebook)" if (s["derating_sustentado_sobre_rajada"] or 1) < 0.97 else "CPU"
        w("| %s | %s | %s | %s | %s | %s%% | %s%% | %s | %s |" % (nomes[k], fmt(sm), fmt(s["rajada_MBps"]), fmtv(s["derating_sustentado_sobre_rajada"]), fmtv(s["PB_mes"]["sustentado"], 1), fmt(100 * s["fracao_de_5PB_mes_media"]["sustentado"]), fmt(100 * s["fracao_de_20PB_mes_pico_x4"]["sustentado"]), lim, obs))
        linhas_json.append({"teste": nomes[k], "MBps_sustentado": sm, "MBps_rajada": s["rajada_MBps"], "derating": s["derating_sustentado_sobre_rajada"], "PB_mes_sustentado": s["PB_mes"]["sustentado"], "fracao_5PB": s["fracao_de_5PB_mes_media"]["sustentado"], "fracao_pico_x4": s["fracao_de_20PB_mes_pico_x4"]["sustentado"], "limitante": lim})
    for k, t in io.get("testes", {}).items():
        lim = t.get("limitante", "-")
        obs = "interrompido: %s" % t["interrompido"]["motivo"] if t.get("interrompido") else "disco R %s / W %s MB/s, util %s%%, cpu_frac %s, %s" % (fmt(t["disco_leitura_MBps"]), fmt(t["disco_escrita_MBps"]), fmt(t["disco_util_pct"]), fmtv(t["cpu_frac_media_por_processo"]), t["variante_saida"])
        w("| I/O: %s | %s | - | - | %s | %s%% | %s%% | %s | %s |" % (k, fmt(t["vazao_entrada_MBps_apos_aquecimento"]), fmtv(t["PB_mes"], 1), fmt(100 * t["fracao_de_5PB_mes_media"]), fmt(100 * t["fracao_de_20PB_mes_pico_x4"]), lim, obs))
        linhas_json.append({"teste": "I/O " + k, "MBps_sustentado": t["vazao_entrada_MBps_apos_aquecimento"], "PB_mes": t["PB_mes"], "fracao_5PB": t["fracao_de_5PB_mes_media"], "fracao_pico_x4": t["fracao_de_20PB_mes_pico_x4"], "limitante": lim})
    w("")
    w("Vazao de descompressao e medida em bytes DESCOMPRIMIDOS por segundo (dado logico), a mesma unidade da entrada da compressao. Nos testes de I/O, a vazao e de bytes logicos de entrada (compressao) ou de saida descomprimida (descompressao), medida depois de um aquecimento inicial.")
    w("")
    w("## 2. Inventario")
    w("")
    if inv:
        w("- Kernel %s; uptime %.0f s no momento do inventario; carga %s." % (inv["kernel"], inv["uptime_s"], inv["loadavg"]))
        f = inv["frequencias_por_tipo"]
        w("- Frequencias (kHz) P: min %s max %s (scaling max %s), E: min %s max %s (scaling max %s). Governor P %s / E %s; EPP P %s / E %s; no_turbo %s; intel_pstate %s." % (f["P"]["cpuinfo_min_freq"], f["P"]["cpuinfo_max_freq"], f["P"]["scaling_max_freq"], f["E"]["cpuinfo_min_freq"], f["E"]["cpuinfo_max_freq"], f["E"]["scaling_max_freq"], f["P"]["scaling_governor"], f["E"]["scaling_governor"], f["P"]["energy_performance_preference"], f["E"]["energy_performance_preference"], inv["intel_pstate"]["no_turbo"], inv["intel_pstate"]["status"]))
        for rot, lista in inv["caches_por_tipo"].items():
            w("- Caches %s: %s" % (rot, "; ".join("L%s %s %s (compart. %s)" % (c["nivel"], c["tipo"], c["tamanho"], c["compartilhado_com"]) for c in lista)))
        rp = inv["rapl"].get("intel-rapl:0", {})
        lim = ", ".join("%s %.0f W" % (rp[k]["nome"], rp[k]["limite_W"]) for k in ("constraint_0", "constraint_1", "constraint_2") if k in rp)
        w("- RAPL: limites do pacote (legiveis): %s; energy_uj legivel: %s. %s." % (lim, rp.get("energy_uj_legivel"), inv["energia_rapl"]))
        w("- AC online: %s; bateria %s (%s%%); powerprofilesctl: %s." % (inv["ac_online"], inv["bateria"]["status"], inv["bateria"]["capacidade"], inv["powerprofilesctl"]))
        m = inv["memoria_MiB"]
        w("- Memoria: total %.0f MiB, disponivel %.0f MiB, swap total %.0f MiB (livre %.0f)." % (m["MemTotal"], m["MemAvailable"], m["SwapTotal"], m["SwapFree"]))
        w("- Conferencia sha256 dos dados contra `medicoes.json`: ok=%s (%s arquivos)." % (inv["conferencia_dados"].get("ok"), inv["conferencia_dados"].get("arquivos_conferidos")))
        w("")
    w("## 3. Calibracao de 1 nucleo (zstd CLI -b1 -e3 -i3, silesia.tar)")
    w("")
    if cal:
        w("Referencia publica (README do zstd 1.5.7, i7-9700K, lzbench, Silesia): nivel 1 razao %.3f, compressao %.0f MB/s, descompressao %.0f MB/s. LIMITACAO: base nao identica (silesia.tar montado aqui, %s bytes; CLI com -T1 e taskset)." % (REF_ZSTD1["razao"], REF_ZSTD1["comp_MBps"], REF_ZSTD1["decomp_MBps"], fmt(cal["tar"]["bytes"])))
        w("")
        w("| nucleo | nivel | razao | compressao MB/s (mediana; min-max) | descompressao MB/s (mediana; min-max) |")
        w("|---|---|---|---|---|")
        for rot in ("P", "E"):
            for nivel, v in cal["por_tipo"][rot].items():
                w("| %s | %s | %s | %s (%s-%s) | %s (%s-%s) |" % (rot, nivel, fmtv(v["razao"], 3), fmt(v["comp_MBps_mediana"], 1), fmt(v["comp_MBps_min"], 1), fmt(v["comp_MBps_max"], 1), fmt(v["decomp_MBps_mediana"], 1), fmt(v["decomp_MBps_min"], 1), fmt(v["decomp_MBps_max"], 1)))
        w("")
        ft = cal["fatores_vs_referencia_publica"]
        w("Fatores (nossa vazao / referencia, nivel 1): P-core CPU 0: compressao %s, descompressao %s, razao %s; E-core CPU 16: compressao %s, descompressao %s. E/P: compressao nivel 1 %s, descompressao nivel 1 %s, compressao nivel 3 %s, descompressao nivel 3 %s." % (fmtv(ft["P"]["comp_fator"], 3), fmtv(ft["P"]["decomp_fator"], 3), fmtv(ft["P"]["razao_nossa_sobre_ref"], 3), fmtv(ft["E"]["comp_fator"], 3), fmtv(ft["E"]["decomp_fator"], 3), fmtv(ft["E_sobre_P"]["comp_nivel1"], 3), fmtv(ft["E_sobre_P"]["decomp_nivel1"], 3), fmtv(ft["E_sobre_P"]["comp_nivel3"], 3), fmtv(ft["E_sobre_P"]["decomp_nivel3"], 3)))
        w("")
    w("## 4. Memoria (helper C, copy e triad estilo STREAM, melhor e mediana de repeticoes)")
    w("")
    if mem:
        w("| configuracao | MiB/vetor/thread | copy melhor GB/s | copy mediana | triad melhor GB/s | triad mediana |")
        w("|---|---|---|---|---|---|")
        for c in mem["configuracoes"]:
            if "erro" in c:
                continue
            w("| %s | %d | %s | %s | %s | %s |" % (c["rotulo"], c["mib_por_vetor_por_thread"], fmtv(c["kernels"]["copy"]["melhor_GBps"], 1), fmtv(c["kernels"]["copy"]["mediana_GBps"], 1), fmtv(c["kernels"]["triad"]["melhor_GBps"], 1), fmtv(c["kernels"]["triad"]["mediana_GBps"], 1)))
        w("")
        w(mem["desvio"] + ". " + mem["bytes_contados"] + ".")
        w("")
        comparacao = []
        mapa = {"A_comp": "8 P", "B_comp": "8 P + 8 E", "C_comp": "32 threads"}
        for k, rot in mapa.items():
            s = sus.get(k)
            m = next((c for c in mem["configuracoes"] if c.get("rotulo") == rot), None)
            if not s or not m or s["sustentado_MBps"] is None:
                continue
            razao = s["setup"]["razao_zstd3_no_conjunto_do_teste"]
            trafego = s["sustentado_MBps"] * (1.0 + 1.0 / razao) / 1000.0
            comparacao.append((rot, trafego, m["kernels"]["copy"]["melhor_GBps"], m["kernels"]["triad"]["melhor_GBps"]))
        if comparacao:
            w("Trafego estimado do zstd-3 na vazao sustentada (entrada + saida por segundo, cada byte lido e escrito uma vez; INFERENCIA PROPRIA: ignora releituras do estado interno do zstd, que ficam em L2/L3, e write-allocate) contra o copy medido:")
            w("")
            w("| config | trafego zstd-3 GB/s | copy melhor GB/s | fracao do copy | triad melhor GB/s |")
            w("|---|---|---|---|---|")
            for rot, t, cp, tr in comparacao:
                w("| %s | %s | %s | %s%% | %s |" % (rot, fmtv(t, 2), fmtv(cp, 1), fmtv(100 * t / cp, 1), fmtv(tr, 1)))
            w("")
    w("## 5. Teste sustentado (10 min por configuracao)")
    w("")
    for k in ordem:
        s = sus.get(k)
        if not s:
            continue
        w("### %s" % nomes[k])
        w("")
        w("- CPUs %s. Rajada (media dos primeiros %.0f s): %s MB/s; sustentado (media dos ultimos %.0f s): %s MB/s; derating %s; media total %s MB/s; minimo/maximo de janelas de 5 s: %s / %s MB/s. Duracao real %.0f s; bit-exatidao no setup: %s; razao zstd-3 do conjunto de blocos do teste: %s." % (",".join(str(c) for c in s["cpus"]), s["janela_rajada_s"], fmt(s["rajada_MBps"]), s["janela_sustentado_s"], fmt(s["sustentado_MBps"]), fmtv(s["derating_sustentado_sobre_rajada"], 3), fmt(s["media_total_MBps"]), fmt(s["min_MBps_janela_5s"]), fmt(s["max_MBps_janela_5s"]), s["duracao_real_s"], s["setup"]["bit_exato"], fmtv(s["setup"]["razao_zstd3_no_conjunto_do_teste"], 3)))
        w("- PB/mes equivalentes: rajada %s, sustentado %s (alvo medio 5; pico x4 sustentado 20)." % (fmtv(s["PB_mes"]["rajada"], 1), fmtv(s["PB_mes"]["sustentado"], 1)))
        fq = s["frequencia_MHz"]
        w("- Frequencia media (MHz): P ativos rajada %s -> sustentado %s; E ativos rajada %s -> sustentado %s; todos os P %s -> %s; todos os E %s -> %s." % (fmt(fq["freq_P_ativos_MHz"]["rajada"]), fmt(fq["freq_P_ativos_MHz"]["sustentado"]), fmt(fq["freq_E_ativos_MHz"]["rajada"]), fmt(fq["freq_E_ativos_MHz"]["sustentado"]), fmt(fq["freq_P_todos_MHz"]["rajada"]), fmt(fq["freq_P_todos_MHz"]["sustentado"]), fmt(fq["freq_E_todos_MHz"]["rajada"]), fmt(fq["freq_E_todos_MHz"]["sustentado"])))
        t = s["temperatura"]
        w("- Temperatura do pacote: inicio %s C, maxima %s C, media dos ultimos %.0f s %s C, fim %s C; maior temperatura de nucleo %s C." % (fmtv(t["pkg_inicio_C"], 0), fmtv(t["pkg_max_C"], 0), s["janela_sustentado_s"], fmtv(t["pkg_media_sustentado_C"], 1), fmtv(t["pkg_fim_C"], 0), fmtv(t["core_max_C"], 0)))
        th = s["throttle"]
        w("- Contadores de throttle (incrementos no teste): core_throttle_count %s (%s ms somados nos 32 CPUs), package_throttle_count %s (%s ms)." % (fmt(th["core_count_incremento"]), fmt(th["core_total_ms_incremento"]), fmt(th["package_count_incremento"]), fmt(th["package_total_ms_incremento"])))
        p = s["potencia"]
        w("- Potencia: %s." % ("rajada %s W, sustentado %s W" % (fmtv(p["rajada_W"], 1), fmtv(p["sustentado_W"], 1)) if p["medida"] else p["motivo"]))
        r = s["ruido_de_fundo"]
        w("- Ruido de fundo (CPU de processos que nao sao nossos, em nucleos-equivalentes nos CPUs do teste e seus irmaos SMT): medio %s, maximo %s; amostras com mais de 0,5 core: %d de %d; mais de 2 cores: %d; inicio %s, fim %s cores no total (todos os CPUs); media no teste, todos os CPUs: %s; maiores (nucleo-segundos): %s; correlacao com a vazao: %s." % (fmtv(r["bg_no_conjunto_cores_medio"], 2), fmtv(r["bg_no_conjunto_cores_max"], 2), r["amostras_com_bg_acima_de_0_5_core"], r["n_amostras"], r["amostras_com_bg_acima_de_2_cores"], fmtv(r["bg_inicio"]["bg_cores_proc"], 2), fmtv(r["bg_fim"]["bg_cores_proc"], 2), fmtv(r["bg_total_cores_medio"], 2), "; ".join("%s %s" % (c, fmtv(v, 0)) for c, v in r["maiores_processos_de_fundo_core_segundos"]), fmtv(r.get("correlacao_bg_conjunto_vs_vazao"), 2)))
        w("- MB/s por minuto: %s." % ", ".join(fmt(x) for x in s["por_minuto_MBps"]))
        if s.get("interrompido"):
            w("- INTERROMPIDO aos %.0f s: %s." % (s["interrompido"]["em_s"], s["interrompido"]["motivo"]))
        w("")
    desc = dados.get("sustentado_descansos", [])
    if desc:
        w("Descansos ociosos entre testes (temperatura do pacote a cada 15 s, C):")
        w("")
        for d in desc:
            w("- antes de %s: %s" % (d.get("antes_de"), ", ".join(fmtv(x, 0) for x in d["temp_pkg_C"])))
        w("")
    w("## 6. I/O de disco (NVMe, O_DIRECT)")
    w("")
    if io.get("entrada"):
        e = io["entrada"]
        w("Arquivo de entrada: %.1f GiB, semente %d, razao zstd-3 em amostra de 1/16 dos blocos: %s; razao exata (todos os blocos, cabecalho de 8 B por bloco incluido): %s. Fontes: %s." % (e["bytes"] / GIB, e["semente"], fmtv(e["razao_zstd3_amostra_1_em_16"], 3), fmtv(io["preparo_comprimido"]["razao_exata_zstd3_blocos_1MiB_com_cabecalho_8B"], 3), ", ".join(e["arquivos_fonte"])))
        w("")
    if io.get("tetos_disco"):
        w("Teto do disco medido com os mesmos processos (somente I/O, O_DIRECT, 1 MiB, fila 1 por processo):")
        w("")
        w("| teste | N | MB/s (entrada logica) | disco leitura MB/s | disco escrita MB/s | util % | fila media | latencia p50/p99 do op (ms) |")
        w("|---|---|---|---|---|---|---|---|")
        for k, t in io["tetos_disco"].items():
            lat = t["latencia"]["lat_leitura"] if "leitura" in k else t["latencia"]["lat_escrita"]
            w("| %s | %d | %s | %s | %s | %s | %s | %s / %s |" % (k, t["n_processos"], fmt(t["vazao_entrada_MBps_apos_aquecimento"]), fmt(t["disco_leitura_MBps"]), fmt(t["disco_escrita_MBps"]), fmt(t["disco_util_pct"]), fmtv(t["disco_fila_media"], 1), fmtv(lat["p50_ms"] if lat else None, 2), fmtv(lat["p99_ms"] if lat else None, 2)))
        w("")
    if io.get("testes"):
        w("| teste | duracao s | vazao entrada MB/s | PB/mes | disco leitura MB/s | disco escrita MB/s | util % | cpu_frac | RSS max MiB | razao | lat. bloco p50/p99 ms | lat. cpu p50/p99 ms | lat. leitura 1 MiB p50/p99 ms | lat. escrita 1 MiB p50/p99 ms | limitante |")
        w("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|")
        for k, t in io["testes"].items():
            L = t["latencia"]
            def pp(c):
                return "%s / %s" % (fmtv(L[c]["p50_ms"], 2), fmtv(L[c]["p99_ms"], 2)) if L.get(c) else "-"
            w("| %s | %.0f | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s | %s |" % (k, t["duracao_real_s"], fmt(t["vazao_entrada_MBps_apos_aquecimento"]), fmtv(t["PB_mes"], 1), fmt(t["disco_leitura_MBps"]), fmt(t["disco_escrita_MBps"]), fmt(t["disco_util_pct"]), fmtv(t["cpu_frac_media_por_processo"]), fmtv(t["rss_max_MiB"], 0), fmtv(t["razao_obtida"], 2), pp("lat_bloco"), pp("lat_cpu"), pp("lat_leitura"), pp("lat_escrita"), t["limitante"]))
        w("")
        w("Variantes: " + "; ".join("%s = %s" % (k, t["variante_saida"]) for k, t in io["testes"].items()) + ".")
        w("")
        w("| teste | vazao 1a metade ate fim MB/s (segunda metade) | disco escrita MB/s (primeiros s -> segunda metade) | disco leitura MB/s (segunda metade) | vazao incluindo flush final MB/s | flush final s | temp. pkg max C | temp. NVMe max C | throttle (core/pkg) |")
        w("|---|---|---|---|---|---|---|---|---|")
        for k, t in io["testes"].items():
            w("| %s | %s | %s -> %s | %s | %s | %s | %s | %s | %s / %s |" % (k, fmt(t["vazao_entrada_MBps_segunda_metade"]), fmt(t["disco_escrita_MBps_primeiros_segundos"]), fmt(t["disco_escrita_MBps_segunda_metade"]), fmt(t["disco_leitura_MBps_segunda_metade"]), fmt(t["vazao_entrada_MBps_incluindo_flush_final"]), fmtv(t["flush_final_s_max"], 1), fmtv(t["temp_pkg_max_C"], 0), fmtv(t["temp_nvme_max_C"], 0), fmt(t["throttle_incremento"]["core_count"]), fmt(t["throttle_incremento"]["package_count"])))
        w("")
        ctl = {n: io["testes"].get("controle_cpu_sem_io_N%d" % n) for n in (8, 16)}
        for n in (8, 16):
            t = io["testes"].get("comp_N%d_odirect" % n)
            if t and ctl[n]:
                w("- Compressao N=%d com I/O (O_DIRECT) = %s MB/s contra %s MB/s do controle sem I/O no mesmo conteudo: o I/O custa %s%% da vazao de CPU." % (n, fmt(t["vazao_entrada_MBps_apos_aquecimento"]), fmt(ctl[n]["vazao_entrada_MBps_apos_aquecimento"]), fmt(100 * (1 - t["vazao_entrada_MBps_apos_aquecimento"] / ctl[n]["vazao_entrada_MBps_apos_aquecimento"]))))
        w("")
        w("Escrita fisica total no NVMe durante a fase (diskstats): %s GiB; leitura fisica %s GiB; pegada maxima em disco %s GiB (limite de 40 GiB lido como espaco ocupado, ver README); limpeza: %s." % (fmt(io.get("escrita_fisica_total_GiB")), fmt(io.get("leitura_fisica_total_GiB")), fmtv(io.get("pegada_maxima_em_disco_GiB"), 1), io.get("limpeza")))
        w("")
    w("## 7. Fatores de calibracao e de extrapolacao (INFERENCIA PROPRIA onde indicado)")
    w("")
    fat = []
    if cal:
        ft = cal["fatores_vs_referencia_publica"]
        fat.append("1 nucleo, nivel 1, vs referencia publica: P-core %sx (compressao), %sx (descompressao); E-core %sx / %sx." % (fmtv(ft["P"]["comp_fator"], 2), fmtv(ft["P"]["decomp_fator"], 2), fmtv(ft["E"]["comp_fator"], 2), fmtv(ft["E"]["decomp_fator"], 2)))
        fat.append("E-core / P-core: %sx (compressao nivel 3), %sx (descompressao nivel 3)." % (fmtv(ft["E_sobre_P"]["comp_nivel3"], 2), fmtv(ft["E_sobre_P"]["decomp_nivel3"], 2)))
    a_ = sus.get("A_comp")
    b_ = sus.get("B_comp")
    c_ = sus.get("C_comp")
    if a_ and a_["sustentado_MBps"]:
        fat.append("Derating sustentado/rajada: A %s, B %s, C %s." % (fmtv(a_["derating_sustentado_sobre_rajada"], 3), fmtv(b_["derating_sustentado_sobre_rajada"], 3) if b_ else "-", fmtv(c_["derating_sustentado_sobre_rajada"], 3) if c_ else "-"))
        fat.append("Por nucleo no sustentado (compressao zstd-3): A %s MB/s por P fisico." % fmt(a_["sustentado_MBps"] / 8))
    if a_ and b_ and a_["sustentado_MBps"] and b_["sustentado_MBps"]:
        fat.append("Ganho de adicionar 8 E-cores a 8 P: %sx sobre A; contribuicao media de cada E: %s MB/s (INFERENCIA PROPRIA: B - A dividido por 8)." % (fmtv(b_["sustentado_MBps"] / a_["sustentado_MBps"], 2), fmt((b_["sustentado_MBps"] - a_["sustentado_MBps"]) / 8)))
    if b_ and c_ and b_["sustentado_MBps"] and c_["sustentado_MBps"]:
        fat.append("Ganho de passar de 16 para 32 threads (mais 8 E e 8 irmaos SMT): %sx sobre B." % fmtv(c_["sustentado_MBps"] / b_["sustentado_MBps"], 2))
    ctl8 = io.get("testes", {}).get("controle_cpu_sem_io_N8")
    if ctl8 and a_ and a_["rajada_MBps"]:
        fc = ctl8["vazao_entrada_MBps_apos_aquecimento"] / a_["rajada_MBps"]
        fat.append("Fator de conteudo: o mix do arquivo de I/O (razao zstd-3 %s) comprime a %s MB/s em 8 P (controle sem I/O, 25 s) contra %s MB/s de rajada do conjunto de blocos do teste sustentado (razao %s): fator %sx. Aplicado ao sustentado A (INFERENCIA PROPRIA, supoe o mesmo derating): %s MB/s no mix de razao ~2,56." % (fmtv(ctl8["razao_obtida"], 2), fmt(ctl8["vazao_entrada_MBps_apos_aquecimento"]), fmt(a_["rajada_MBps"]), fmtv(a_["setup"]["razao_zstd3_no_conjunto_do_teste"], 2), fmtv(fc, 3), fmt(a_["sustentado_MBps"] * fc)))
    for f in fat:
        w("- " + f)
    w("")
    resumo_json = {"linhas": linhas_json, "alvo_media_MBps": ALVO_MEDIA_MBPS, "alvo_pico_x4_MBps": ALVO_PICO_MBPS, "fatores": fat}
    gravar_secao(a.saida, "resumo", resumo_json)
    with open(a.md, "w") as f:
        f.write("\n".join(linhas) + "\n")
    log("resumo gravado em %s" % a.md)


def cmd_tudo(a):
    inicio = time.time()
    cmd_inventario(a)
    cmd_calibracao(a)
    cmd_memoria(a)
    a.descanso_inicial = True
    cmd_sustentado(a)
    if not a.sem_io:
        am = Amostrador(list(range(32)), os.getpid())
        gravar_secao(a.saida, "descanso_antes_io", descansar(a.io_descanso_inicial, am))
        cmd_io_disco(a)
    cmd_resumo(a)
    gravar_secao(a.saida, "tempo_total_s", time.time() - inicio)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("comando", choices=["inventario", "calibracao", "memoria", "sustentado", "io-disco", "resumo", "tudo"])
    ap.add_argument("--dados", required=True)
    ap.add_argument("--trabalho", required=True)
    ap.add_argument("--saida", default=os.path.join(BASE, "resultados", "capacidade_local.json"))
    ap.add_argument("--md", default=os.path.join(BASE, "resultados", "capacidade_local.md"))
    ap.add_argument("--repeticoes", type=int, default=3)
    ap.add_argument("--mem-repeticoes", type=int, default=5)
    ap.add_argument("--teto-gib", type=float, default=6.0)
    ap.add_argument("--duracao", type=float, default=600.0)
    ap.add_argument("--intervalo", type=float, default=5.0)
    ap.add_argument("--descanso", type=float, default=180.0)
    ap.add_argument("--descanso-inicial", action="store_true")
    ap.add_argument("--descanso-antes-a", type=float, default=120.0)
    ap.add_argument("--configs", default="A_comp,B_comp,C_comp,B_decomp")
    ap.add_argument("--io-dir", default=CACHE_IO)
    ap.add_argument("--io-gib", type=float, default=16.0)
    ap.add_argument("--io-cap-mib", type=int, default=512)
    ap.add_argument("--io-teto-leitura", type=float, default=10.0)
    ap.add_argument("--io-teto-escrita", type=float, default=60.0)
    ap.add_argument("--io-descanso", type=float, default=30.0)
    ap.add_argument("--io-descanso-inicial", type=float, default=60.0)
    ap.add_argument("--io-plano", default="comp:8:odirect:180,comp:16:odirect:180,comp:16:bufferizado:60,ram:8:cpu:25,ram:16:cpu:25,decomp:16:descartar:90,decomp:16:grava:90,decomp:8:descartar:45")
    ap.add_argument("--sem-io", action="store_true")
    a = ap.parse_args()
    if a.comando == "inventario":
        cmd_inventario(a)
    elif a.comando == "calibracao":
        cmd_calibracao(a)
    elif a.comando == "memoria":
        cmd_memoria(a)
    elif a.comando == "sustentado":
        cmd_sustentado(a)
    elif a.comando == "io-disco":
        cmd_io_disco(a)
    elif a.comando == "resumo":
        cmd_resumo(a)
    else:
        cmd_tudo(a)


if __name__ == "__main__":
    main()
