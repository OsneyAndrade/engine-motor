import argparse
import json
import os


def fmt(x, casas=0):
    return ("%." + str(casas) + "f") % x


def tabela(w, grupo, config):
    w("| N | CPUs | comp agregado MB/s | comp por nucleo MB/s | ef comp (vs mesmo tipo) | ef comp (vs P-core, definicao anterior) | decomp agregado MB/s | decomp por nucleo MB/s | ef decomp (vs mesmo tipo) | ef decomp (vs P-core) |")
    w("|---|---|---|---|---|---|---|---|---|---|")
    for l in grupo["configs"][config]["por_n"]:
        c, d = l["comp"], l["decomp"]
        cpus = ",".join(str(x) for x in l["cpus"])
        w("| %d | %s | %s | %s | %s | %s | %s | %s | %s | %s |" % (l["n"], cpus, fmt(c["agregado_MBps_mediana"]), fmt(c["por_nucleo_MBps_mediana"]), fmt(c["eficiencia_vs_mesmo_tipo"], 2), fmt(c["eficiencia_vs_P_core"], 2), fmt(d["agregado_MBps_mediana"]), fmt(d["por_nucleo_MBps_mediana"]), fmt(d["eficiencia_vs_mesmo_tipo"], 2), fmt(d["eficiencia_vs_P_core"], 2)))
    w("")


def linha_n(grupo, config, n):
    for l in grupo["configs"][config]["por_n"]:
        if l["n"] == n:
            return l
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--entrada", required=True)
    ap.add_argument("--medicoes-anteriores", default="")
    ap.add_argument("--md", required=True)
    a = ap.parse_args()
    with open(a.entrada) as f:
        dados = json.load(f)
    esc = dados["escala_cpus"]
    linhas = []
    w = linhas.append
    w("# escala em nucleos homogeneos (zstd-3 e zstd-1, blocos de 1 MiB, processos independentes)")
    w("")
    w("Gerado por `bench/escala_resumo.py` a partir de `resultados/escala_homogenea.json`. Mesma metodologia da fase `escala` de `medicoes.json`: dados distintos por processo, barreira, rodadas de %.1f s, 1 rodada de aquecimento descartada, mediana de %d rodadas. CPU: %s." % (esc["duracao_s"], dados["protocolo"]["repeticoes"], dados["meta"]["cpu_modelo"]))
    w("")
    w("Eficiencia (vs mesmo tipo) = agregado medido / soma da vazao isolada de cada trabalhador medida em um nucleo do MESMO tipo (CPU %d para P, CPU %d para E). Eficiencia (vs P-core) = a definicao da curva anterior (todo trabalhador comparado ao P-core isolado)." % (esc["cpu_referencia_P_isolado"], esc["cpu_referencia_E_isolado"]))
    w("")
    w("## Vazao isolada (1 nucleo), pool inteiro")
    w("")
    w("| config | P comp MB/s | E comp MB/s | E/P comp | P decomp MB/s | E decomp MB/s | E/P decomp |")
    w("|---|---|---|---|---|---|---|")
    for cfg, v in esc["isolado"].items():
        w("| %s | %s | %s | %s | %s | %s | %s |" % (cfg, fmt(v["P"]["comp_MBps"]), fmt(v["E"]["comp_MBps"]), fmt(v["razao_E_sobre_P_comp"], 3), fmt(v["P"]["decomp_MBps"]), fmt(v["E"]["decomp_MBps"]), fmt(v["razao_E_sobre_P_decomp"], 3)))
    w("")
    nomes = {"E": "(a) so E-cores fisicos", "P": "(b) so P-cores fisicos", "PSMT": "(c) P-cores com SMT", "PE": "(d) 8 P fisicos + 8 E"}
    for rot, g in esc["grupos"].items():
        for cfg in g["configs"]:
            w("## %s, %s" % (nomes.get(rot, rot), cfg))
            w("")
            tabela(w, g, cfg)
    w("## Derivados")
    w("")
    for cfg in ("zstd-3_bloco_1MiB", "zstd-1_bloco_1MiB"):
        w("### %s" % cfg)
        w("")
        g = esc["grupos"]
        if "P" in g and "PSMT" in g:
            p8 = linha_n(g["P"], cfg, 8)
            s16 = linha_n(g["PSMT"], cfg, 16)
            s8 = linha_n(g["PSMT"], cfg, 8)
            if p8 and s16:
                w("- Ganho do SMT: 16 threads em 8 P-cores / 8 threads em 8 P-cores fisicos: comp %.3fx (%.0f -> %.0f MB/s), decomp %.3fx (%.0f -> %.0f MB/s)." % (s16["comp"]["agregado_MBps_mediana"] / p8["comp"]["agregado_MBps_mediana"], p8["comp"]["agregado_MBps_mediana"], s16["comp"]["agregado_MBps_mediana"], s16["decomp"]["agregado_MBps_mediana"] / p8["decomp"]["agregado_MBps_mediana"], p8["decomp"]["agregado_MBps_mediana"], s16["decomp"]["agregado_MBps_mediana"]))
            if p8 and s8:
                w("- Repetibilidade: N=8 em 8 P fisicos medido duas vezes (grupos P e PSMT): comp %.0f vs %.0f MB/s, decomp %.0f vs %.0f MB/s." % (p8["comp"]["agregado_MBps_mediana"], s8["comp"]["agregado_MBps_mediana"], p8["decomp"]["agregado_MBps_mediana"], s8["decomp"]["agregado_MBps_mediana"]))
        if "E" in g:
            e16 = linha_n(g["E"], cfg, 16)
            if e16:
                w("- 16 E-cores homogeneos: eficiencia vs 1 E-core, comp %.2f, decomp %.2f." % (e16["comp"]["eficiencia_vs_mesmo_tipo"], e16["decomp"]["eficiencia_vs_mesmo_tipo"]))
        if "PE" in g:
            m16 = linha_n(g["PE"], cfg, 16)
            if m16:
                w("- 8 P + 8 E (N=16 misto): eficiencia vs P-core (definicao anterior) comp %.2f, decomp %.2f; vs mesmo tipo comp %.2f, decomp %.2f." % (m16["comp"]["eficiencia_vs_P_core"], m16["decomp"]["eficiencia_vs_P_core"], m16["comp"]["eficiencia_vs_mesmo_tipo"], m16["decomp"]["eficiencia_vs_mesmo_tipo"]))
        w("")
    if a.medicoes_anteriores and os.path.exists(a.medicoes_anteriores):
        with open(a.medicoes_anteriores) as f:
            ant = json.load(f)
        w("## Curva anterior (medicoes.json, somente leitura)")
        w("")
        w("| config | N | comp MB/s | ef comp | decomp MB/s | ef decomp |")
        w("|---|---|---|---|---|---|")
        for cfg, v in ant["escala"]["configs"].items():
            for l in v["por_n"]:
                if l["n"] in (1, 8, 16, 24, 32):
                    w("| %s | %d | %s | %s | %s | %s |" % (cfg, l["n"], fmt(l["comp"]["agregado_MBps_mediana"]), fmt(l["comp"]["eficiencia"], 2), fmt(l["decomp"]["agregado_MBps_mediana"]), fmt(l["decomp"]["eficiencia"], 2)))
        w("")
    with open(a.md, "w") as f:
        f.write("\n".join(linhas) + "\n")


if __name__ == "__main__":
    main()
