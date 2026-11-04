import argparse
import json
import math
import os
import sys

GB_H_POR_MBPS = 3.6
CAMADAS = ["quente", "morno", "frio"]
CPUS = ["nuvem", "onprem"]
ARMAZENAMENTOS = ["s3_standard", "b2", "glacier_ir", "onprem"]
CENARIOS_DESTAQUE = ["nuvem+s3_standard", "nuvem+glacier_ir", "onprem+onprem"]
ORDEM_CLASSES = [
    "texto",
    "codigo",
    "binario",
    "cientifico",
    "banco_de_dados",
    "xml",
    "dna",
    "sint_logs",
    "sint_incompressivel",
    "sint_ja_comprimido",
    "mix_real",
]


def val(p, *caminho):
    x = p
    for c in caminho:
        x = x[c]
    return x["valor"]


def carregar(caminho):
    with open(caminho) as f:
        return json.load(f)


def derivar_onprem(p):
    d = p["onprem_derivacao"]
    horas_uteis = val(d, "vida_util_anos") * 8760 * val(d, "utilizacao")
    capex = val(d, "capex_servidor_usd") / val(d, "nucleos_servidor") / horas_uteis
    energia = val(d, "watts_por_nucleo") / 1000.0 * val(d, "pue") * val(d, "usd_por_kwh")
    return capex + energia


def tabela_eficiencia(med, chave):
    if med is None or "escala" not in med or chave not in med["escala"]["configs"]:
        return {"comp": [(1, 1.0)], "decomp": [(1, 1.0)], "origem": "ideal (sem medicao de escala)"}
    pts = med["escala"]["configs"][chave]["por_n"]
    return {
        "comp": [(l["n"], min(1.0, l["comp"]["eficiencia"])) for l in pts],
        "decomp": [(l["n"], min(1.0, l["decomp"]["eficiencia"])) for l in pts],
        "origem": chave,
    }


def eficiencia(tab, n):
    if n <= tab[0][0]:
        return tab[0][1]
    for (n0, e0), (n1, e1) in zip(tab, tab[1:]):
        if n <= n1:
            return e0 + (e1 - e0) * (n - n0) / (n1 - n0)
    return tab[-1][1]


def nucleos(rho_mbps, theta, fator, tab):
    n = rho_mbps / (theta * fator)
    e = eficiencia(tab, n)
    for _ in range(200):
        e = eficiencia(tab, n)
        novo = rho_mbps / (theta * fator * e)
        if abs(novo - n) < 1e-9:
            n = novo
            break
        n = novo
    return n, e


class Contexto:
    def __init__(self, p, med, n_max=None, fator=None):
        self.p = p
        self.volume_b = val(p, "volume_mensal_bytes")
        self.volume_gb = self.volume_b / 1e9
        self.rho_mbps = self.volume_b / (val(p, "dias_mes_vazao") * 86400) / 1e6
        self.k = val(p, "k_pico")
        self.rho_pico_mbps = self.k * self.rho_mbps
        self.horas = val(p, "horas_mes_cobranca")
        self.T = val(p, "retencao_meses")
        self.red = val(p, "redundancia_erasure")
        self.fator = fator if fator is not None else val(p, "fator_vazao_servidor_vs_notebook")
        self.nuc_srv = val(p, "nucleos_por_servidor")
        self.srv_rack = val(p, "servidores_por_rack")
        self.preco_cpu = {c: val(p, "preco_nucleo_hora", c) for c in CPUS}
        self.preco_arm = {a: val(p, "preco_gb_mes", a) for a in ARMAZENAMENTOS}
        self.n_leituras = {c: val(p, "camadas_leitura", c) for c in CAMADAS}
        self.tab = tabela_eficiencia(med, val(p, "escala_config_referencia"))
        if n_max is not None:
            self.tab = dict(self.tab)
            for k in ("comp", "decomp"):
                self.tab[k] = [pt for pt in self.tab[k] if pt[0] <= n_max]
        self.cenarios = [(c, a) for c in CPUS for a in ARMAZENAMENTOS]

    def custo_gb(self, preco, theta, ef):
        return preco / (theta * self.fator * ef * GB_H_POR_MBPS)

    def perfil(self, theta_c, theta_d, s, stored):
        n_med, ef_c = nucleos(self.rho_mbps, theta_c, self.fator, self.tab["comp"])
        n_pico, ef_cp = nucleos(self.rho_pico_mbps, theta_c, self.fator, self.tab["comp"])
        ef_d = eficiencia(self.tab["decomp"], n_med)
        n_pico_d, _ = nucleos(self.rho_pico_mbps, theta_d, self.fator, self.tab["decomp"])
        if stored:
            n_med = n_pico = n_pico_d = 0.0
        cru = s <= 0 or stored
        saida = {
            "N_medio": n_med,
            "N_pico": n_pico,
            "N_pico_inteiro": math.ceil(n_pico - 1e-9),
            "N_pico_decomp_mesmo_volume": n_pico_d,
            "eficiencia_escala_comp": ef_c,
            "eficiencia_escala_comp_no_pico": ef_cp,
            "eficiencia_escala_decomp": ef_d,
            "cabe_servidor_64": math.ceil(n_pico - 1e-9) <= self.nuc_srv,
            "servidores_no_pico": math.ceil(math.ceil(n_pico - 1e-9) / self.nuc_srv),
            "cabe_1_rack": math.ceil(n_pico - 1e-9) <= self.nuc_srv * self.srv_rack,
            "armazenado_cru": cru,
        }
        for c in CPUS:
            pc = self.preco_cpu[c]
            saida[c] = {
                "custo_mensal_usd": n_med * pc * self.horas,
                "custo_mensal_provisionado_pico_usd": saida["N_pico_inteiro"] * pc * self.horas,
                "custo_comp_usd_por_gb": 0.0 if stored else self.custo_gb(pc, theta_c, ef_c),
                "custo_decomp_usd_por_gb": 0.0 if cru else self.custo_gb(pc, theta_d, ef_d),
            }
        return saida

    def liquido_gb(self, s, perf, cpu, arm, camada):
        s_ef = max(s, 0.0)
        valor = s_ef * self.preco_arm[arm] * self.T * self.red
        cc = perf[cpu]["custo_comp_usd_por_gb"]
        cd = perf[cpu]["custo_decomp_usd_por_gb"]
        n = self.n_leituras[camada]
        return {
            "valor_armazenamento_usd_gb": valor,
            "custo_comp_usd_gb": cc,
            "custo_decomp_por_leitura_usd_gb": cd,
            "n_leituras": n,
            "liquido_usd_gb": valor - cc - n * cd,
            "liquido_mensal_coorte_usd": (valor - cc - n * cd) * self.volume_gb,
        }


def agregar(med):
    sint = {a["nome"]: a["sintetico"] for a in med["arquivos"]}
    acc = {}
    for x in med["principal"]:
        classes = [x["classe"]]
        if not sint.get(x["arquivo"], False):
            classes.append("mix_real")
        for c in classes:
            k = (c, x["modo"], x["config"])
            a = acc.setdefault(k, {"orig": 0, "comp": 0, "tc": 0.0, "td": 0.0, "n": 0, "exato": True, "arqs": []})
            a["orig"] += x["bytes_orig"]
            a["comp"] += x["bytes_comp"]
            a["tc"] += x["comp"]["mediana_s"]
            a["td"] += x["decomp"]["mediana_s"]
            a["n"] += 1
            a["exato"] = a["exato"] and x["bit_exato"]
            a["arqs"].append(x["arquivo"])
    saida = []
    for (c, modo, cfg), a in acc.items():
        s = 1.0 - a["comp"] / a["orig"]
        saida.append(
            {
                "classe": c,
                "modo": modo,
                "config": cfg,
                "bytes_orig": a["orig"],
                "bytes_comp": a["comp"],
                "razao_x": a["orig"] / a["comp"],
                "fracao_economizada": s,
                "theta_c_MBps": a["orig"] / 1e6 / a["tc"],
                "theta_d_MBps": a["orig"] / 1e6 / a["td"],
                "n_arquivos": a["n"],
                "arquivos": sorted(set(a["arqs"])),
                "bit_exato": a["exato"],
            }
        )
    return saida


def pareto(recs, chave):
    marcados = {}
    for r in recs:
        dominado = False
        for o in recs:
            if o is r:
                continue
            if o["fracao_economizada"] >= r["fracao_economizada"] and o[chave] >= r[chave]:
                if o["fracao_economizada"] > r["fracao_economizada"] or o[chave] > r[chave]:
                    dominado = True
                    break
        marcados[r["config"]] = not dominado
    return marcados


def calcular(med, p):
    ctx = Contexto(p, med)
    recs = agregar(med)
    grupos = {}
    for r in recs:
        grupos.setdefault((r["classe"], r["modo"]), []).append(r)

    for (classe, modo), lista in grupos.items():
        pc = pareto(lista, "theta_c_MBps")
        pd = pareto(lista, "theta_d_MBps")
        for r in lista:
            r["perfil"] = ctx.perfil(r["theta_c_MBps"], r["theta_d_MBps"], r["fracao_economizada"], r["config"] == "stored")
            r["pareto_razao_x_vazao_comp"] = pc[r["config"]]
            r["pareto_razao_x_vazao_decomp"] = pd[r["config"]]
            r["cumpre_5PB_mes_com_64_nucleos_no_pico"] = r["perfil"]["cabe_servidor_64"]

    liquido = []
    otimo = []
    otimo_restrito = []
    marginal = []
    for (classe, modo), lista in grupos.items():
        for cpu, arm in ctx.cenarios:
            cen = cpu + "+" + arm
            for camada in CAMADAS:
                linhas = []
                for r in lista:
                    l = ctx.liquido_gb(r["fracao_economizada"], r["perfil"], cpu, arm, camada)
                    l.update({"classe": classe, "modo": modo, "config": r["config"], "cenario": cen, "camada": camada})
                    linhas.append(l)
                    liquido.append(l)
                melhor = max(linhas, key=lambda l: l["liquido_usd_gb"])
                rm = next(r for r in lista if r["config"] == melhor["config"])
                otimo.append(
                    {
                        "classe": classe,
                        "modo": modo,
                        "cenario": cen,
                        "camada": camada,
                        "config": melhor["config"],
                        "liquido_usd_gb": melhor["liquido_usd_gb"],
                        "liquido_mensal_coorte_usd": melhor["liquido_mensal_coorte_usd"],
                        "razao_x": rm["razao_x"],
                        "theta_c_MBps": rm["theta_c_MBps"],
                        "theta_d_MBps": rm["theta_d_MBps"],
                        "N_pico": rm["perfil"]["N_pico"],
                        "custo_mensal_cpu_usd": rm["perfil"][cpu]["custo_mensal_usd"],
                    }
                )
                cabem = {r["config"] for r in lista if r["perfil"]["cabe_servidor_64"]}
                melhor_r = max((l for l in linhas if l["config"] in cabem), key=lambda l: l["liquido_usd_gb"])
                rr = next(r for r in lista if r["config"] == melhor_r["config"])
                otimo_restrito.append(
                    {
                        "classe": classe,
                        "modo": modo,
                        "cenario": cen,
                        "camada": camada,
                        "config": melhor_r["config"],
                        "liquido_usd_gb": melhor_r["liquido_usd_gb"],
                        "liquido_mensal_coorte_usd": melhor_r["liquido_mensal_coorte_usd"],
                        "razao_x": rr["razao_x"],
                        "theta_c_MBps": rr["theta_c_MBps"],
                        "theta_d_MBps": rr["theta_d_MBps"],
                        "N_pico": rr["perfil"]["N_pico"],
                        "custo_mensal_cpu_usd": rr["perfil"][cpu]["custo_mensal_usd"],
                        "perda_vs_otimo_irrestrito_usd_gb": melhor["liquido_usd_gb"] - melhor_r["liquido_usd_gb"],
                    }
                )
                ordenadas = sorted(lista, key=lambda r: (max(r["fracao_economizada"], 0.0), -r["theta_c_MBps"]))
                passos = []
                for a_, b_ in zip(ordenadas, ordenadas[1:]):
                    la = next(l for l in linhas if l["config"] == a_["config"])
                    lb = next(l for l in linhas if l["config"] == b_["config"])
                    d_valor = lb["valor_armazenamento_usd_gb"] - la["valor_armazenamento_usd_gb"]
                    d_cpu = (lb["custo_comp_usd_gb"] + lb["n_leituras"] * lb["custo_decomp_por_leitura_usd_gb"]) - (
                        la["custo_comp_usd_gb"] + la["n_leituras"] * la["custo_decomp_por_leitura_usd_gb"]
                    )
                    passos.append(
                        {
                            "de": a_["config"],
                            "para": b_["config"],
                            "delta_fracao_economizada": max(b_["fracao_economizada"], 0.0) - max(a_["fracao_economizada"], 0.0),
                            "ganho_armazenamento_usd_gb": d_valor,
                            "custo_cpu_marginal_usd_gb": d_cpu,
                            "delta_liquido_usd_gb": d_valor - d_cpu,
                            "razao_ganho_custo": (d_valor / d_cpu) if d_cpu > 0 else None,
                            "vale_a_pena": d_valor - d_cpu > 0,
                        }
                    )
                marginal.append({"classe": classe, "modo": modo, "cenario": cen, "camada": camada, "ordem_por_razao": [r["config"] for r in ordenadas], "passos": passos})

    recompressao = recompressao_regra(ctx, grupos, p)
    return ctx, recs, grupos, liquido, otimo, marginal, recompressao, otimo_restrito


def recompressao_regra(ctx, grupos, p):
    origens = val(p, "recompressao", "origens")
    destinos = val(p, "recompressao", "destinos")
    vidas = val(p, "recompressao", "vidas_restantes_meses")
    ef_c_cheio = eficiencia(ctx.tab["comp"], ctx.tab["comp"][-1][0])
    ef_d_cheio = eficiencia(ctx.tab["decomp"], ctx.tab["decomp"][-1][0])
    saida = []
    for (classe, modo), lista in grupos.items():
        por_cfg = {r["config"]: r for r in lista}
        for o in origens:
            if o not in por_cfg:
                continue
            ro = por_cfg[o]
            for d in destinos:
                if d not in por_cfg or d == o:
                    continue
                rd = por_cfg[d]
                ds = max(rd["fracao_economizada"], 0.0) - max(ro["fracao_economizada"], 0.0)
                for cpu, arm in ctx.cenarios:
                    pc = ctx.preco_cpu[cpu]
                    cd_o = ctx.custo_gb(pc, ro["theta_d_MBps"], ef_d_cheio)
                    cd_d = ctx.custo_gb(pc, rd["theta_d_MBps"], ef_d_cheio)
                    cc_d = ctx.custo_gb(pc, rd["theta_c_MBps"], ef_c_cheio)
                    custo_repack = cd_o + cc_d
                    ganho_mes = ds * ctx.preco_arm[arm] * ctx.red
                    for camada in CAMADAS:
                        n = ctx.n_leituras[camada]
                        taxa = n / ctx.T
                        dcd = cd_d - cd_o
                        denom = ganho_mes - taxa * dcd
                        entrada = {
                            "classe": classe,
                            "modo": modo,
                            "cenario": cpu + "+" + arm,
                            "camada": camada,
                            "origem": o,
                            "destino": d,
                            "delta_fracao_economizada": ds,
                            "custo_repack_usd_gb": custo_repack,
                            "ganho_armazenamento_usd_gb_mes": ganho_mes,
                            "delta_custo_leitura_usd_gb": dcd,
                            "leituras_por_mes": taxa,
                            "meses_equilibrio": (custo_repack / denom) if denom > 0 and ds > 0 else None,
                            "vale_algum_dia": denom > 0 and ds > 0,
                            "n_max_leituras_na_vida_restante": {},
                        }
                        for m in vidas:
                            bruto = ganho_mes * m - custo_repack
                            if bruto <= 0 or ds <= 0:
                                entrada["n_max_leituras_na_vida_restante"][str(m)] = 0.0 if ds > 0 else None
                            elif dcd <= 0:
                                entrada["n_max_leituras_na_vida_restante"][str(m)] = "ilimitado"
                            else:
                                entrada["n_max_leituras_na_vida_restante"][str(m)] = bruto / dcd
                        saida.append(entrada)
    return saida


def penalidade_bloco(recs):
    idx = {(r["classe"], r["modo"], r["config"]): r for r in recs}
    saida = []
    for (classe, modo, cfg), r in idx.items():
        if modo == "inteiro":
            continue
        base = idx.get((classe, "inteiro", cfg))
        if base is None or cfg == "stored":
            continue
        saida.append(
            {
                "classe": classe,
                "modo": modo,
                "config": cfg,
                "razao_x_inteiro": base["razao_x"],
                "razao_x_bloco": r["razao_x"],
                "aumento_tamanho_pct": (r["bytes_comp"] / base["bytes_comp"] - 1.0) * 100.0,
                "razao_bloco_sobre_inteiro": r["razao_x"] / base["razao_x"],
            }
        )
    return saida


def dicionario_ganho(ctx, med):
    if not med.get("dicionario"):
        return []
    acc = {}
    for x in med["dicionario"]:
        k = (x["classe"], x["config"], x["com_dicionario"])
        a = acc.setdefault(k, {"orig": 0, "comp": 0, "tc": 0.0, "td": 0.0, "arqs": [], "dic": 0, "exato": True})
        a["orig"] += x["bytes_orig"]
        a["comp"] += x["bytes_comp"]
        a["tc"] += x["comp"]["mediana_s"]
        a["td"] += x["decomp"]["mediana_s"]
        a["arqs"].append(x["arquivo"])
        a["dic"] = max(a["dic"], x["tamanho_dicionario"])
        a["exato"] = a["exato"] and x["bit_exato"]
    saida = []
    for (classe, cfg, com), a in sorted(acc.items()):
        if com:
            continue
        b = acc[(classe, cfg, True)]
        s0 = 1 - a["comp"] / a["orig"]
        s1 = 1 - b["comp"] / b["orig"]
        reg = {
            "classe": classe,
            "config": cfg,
            "arquivos": sorted(set(a["arqs"])),
            "bytes_teste": a["orig"],
            "tamanho_dicionario": b["dic"],
            "razao_x_sem": a["orig"] / a["comp"],
            "razao_x_com": b["orig"] / b["comp"],
            "ganho_razao_pct": (a["comp"] / b["comp"] - 1) * 100,
            "reducao_tamanho_pct": (1 - b["comp"] / a["comp"]) * 100,
            "theta_c_sem_MBps": a["orig"] / 1e6 / a["tc"],
            "theta_c_com_MBps": b["orig"] / 1e6 / b["tc"],
            "theta_d_sem_MBps": a["orig"] / 1e6 / a["td"],
            "theta_d_com_MBps": b["orig"] / 1e6 / b["td"],
            "bit_exato": a["exato"] and b["exato"],
            "delta_liquido_usd_gb": {},
        }
        p0 = ctx.perfil(reg["theta_c_sem_MBps"], reg["theta_d_sem_MBps"], s0, False)
        p1 = ctx.perfil(reg["theta_c_com_MBps"], reg["theta_d_com_MBps"], s1, False)
        for cpu, arm in ctx.cenarios:
            for camada in CAMADAS:
                l0 = ctx.liquido_gb(s0, p0, cpu, arm, camada)["liquido_usd_gb"]
                l1 = ctx.liquido_gb(s1, p1, cpu, arm, camada)["liquido_usd_gb"]
                reg["delta_liquido_usd_gb"]["%s+%s|%s" % (cpu, arm, camada)] = l1 - l0
        saida.append(reg)
    return saida


def ecore_e_vies(med):
    if not med.get("ecore"):
        return [], []
    acc = {}
    for x in med["ecore"]:
        k = (x["tipo_nucleo"], x["modo"], x["config"])
        a = acc.setdefault(k, {"orig": 0, "tc": 0.0, "td": 0.0})
        a["orig"] += x["bytes_orig"]
        a["tc"] += x["comp"]["mediana_s"]
        a["td"] += x["decomp"]["mediana_s"]
    comparacao = []
    chaves = sorted({(m, c) for (_, m, c) in acc})
    for m, c in chaves:
        pa = acc.get(("P_isolado", m, c))
        ea = acc.get(("E_isolado", m, c))
        if not pa or not ea:
            continue
        comparacao.append(
            {
                "modo": m,
                "config": c,
                "P_comp_MBps": pa["orig"] / 1e6 / pa["tc"],
                "E_comp_MBps": ea["orig"] / 1e6 / ea["tc"],
                "P_decomp_MBps": pa["orig"] / 1e6 / pa["td"],
                "E_decomp_MBps": ea["orig"] / 1e6 / ea["td"],
                "E_sobre_P_comp": (ea["orig"] / ea["tc"]) / (pa["orig"] / pa["tc"]),
                "E_sobre_P_decomp": (ea["orig"] / ea["td"]) / (pa["orig"] / pa["td"]),
            }
        )
    principal = {}
    for x in med["principal"]:
        principal[(x["arquivo"], x["modo"], x["config"])] = x
    vies = []
    por_cfg = {}
    for x in med["ecore"]:
        if x["tipo_nucleo"] != "P_isolado":
            continue
        pr = principal.get((x["arquivo"], x["modo"], x["config"]))
        if pr is None:
            continue
        k = (x["modo"], x["config"])
        a = por_cfg.setdefault(k, {"orig": 0, "tc_iso": 0.0, "td_iso": 0.0, "tc_par": 0.0, "td_par": 0.0})
        a["orig"] += x["bytes_orig"]
        a["tc_iso"] += x["comp"]["mediana_s"]
        a["td_iso"] += x["decomp"]["mediana_s"]
        a["tc_par"] += pr["comp"]["mediana_s"]
        a["td_par"] += pr["decomp"]["mediana_s"]
    for (m, c), a in sorted(por_cfg.items()):
        vies.append(
            {
                "modo": m,
                "config": c,
                "comp_paralelo_sobre_isolado": a["tc_iso"] / a["tc_par"],
                "decomp_paralelo_sobre_isolado": a["td_iso"] / a["td_par"],
            }
        )
    return comparacao, vies


def sensibilidade(p, med, recs):
    modo = val(p, "modo_principal")
    saida = []
    for rotulo, n_max, fator in (("P-cores fisicos (N<=8), fator 1,0", 8, 1.0), ("N<=16, fator 1,0", 16, 1.0), ("N<=32 (base), fator 1,0", 32, 1.0), ("N<=32, fator 0,6", 32, 0.6)):
        c2 = Contexto(p, med, n_max, fator)
        for r in recs:
            if r["modo"] != modo or r["config"] not in ("zstd-1", "zstd-3", "zstd-9", "zstd-19", "xz-9e"):
                continue
            pf = c2.perfil(r["theta_c_MBps"], r["theta_d_MBps"], r["fracao_economizada"], False)
            saida.append(
                {
                    "cenario_escala": rotulo,
                    "n_max_eficiencia": n_max,
                    "fator_vazao_servidor": fator,
                    "classe": r["classe"],
                    "config": r["config"],
                    "eficiencia_comp_no_pico": pf["eficiencia_escala_comp_no_pico"],
                    "N_medio": pf["N_medio"],
                    "N_pico": pf["N_pico"],
                    "cabe_servidor_64": pf["cabe_servidor_64"],
                    "custo_mensal_nuvem_usd": pf["nuvem"]["custo_mensal_usd"],
                }
            )
    return saida


def autoteste(p):
    ctx = Contexto(p, None)
    perf = ctx.perfil(510.0, 1500.0, 0.5, False)
    rho = 5e15 / (30 * 86400) / 1e6
    esperado_n = rho / 510.0
    esperado_nuvem = esperado_n * 0.0446 * 730
    esperado_onprem = esperado_n * 0.0135 * 730
    esperado_pico = math.ceil(4 * esperado_n)
    esperado_cc = 0.0446 / (510.0 * 3.6)
    ok = (
        abs(perf["N_medio"] - 3.7824) < 0.001
        and abs(perf["nuvem"]["custo_mensal_usd"] - 123.14) < 0.1
        and abs(perf["onprem"]["custo_mensal_usd"] - 37.28) < 0.05
        and perf["N_pico_inteiro"] == 16
        and abs(perf["nuvem"]["custo_comp_usd_por_gb"] - esperado_cc) < 1e-12
        and abs(perf["N_medio"] - esperado_n) < 1e-9
        and abs(perf["nuvem"]["custo_mensal_usd"] - esperado_nuvem) < 1e-6
        and abs(perf["onprem"]["custo_mensal_usd"] - esperado_onprem) < 1e-6
        and perf["N_pico_inteiro"] == esperado_pico
    )
    derivado = derivar_onprem(p)
    return {
        "caso": "zstd-1 a 510 MB/s/nucleo, eficiencia de escala 1",
        "rho_MBps": rho,
        "N_medio_programa": perf["N_medio"],
        "N_medio_a_mao": 1929.0123 / 510.0,
        "N_pico_programa": perf["N_pico"],
        "N_pico_inteiro_programa": perf["N_pico_inteiro"],
        "custo_mensal_nuvem_programa_usd": perf["nuvem"]["custo_mensal_usd"],
        "custo_mensal_nuvem_a_mao_usd": 3.7824 * 0.0446 * 730,
        "custo_mensal_onprem_programa_usd": perf["onprem"]["custo_mensal_usd"],
        "custo_mensal_onprem_a_mao_usd": 3.7824 * 0.0135 * 730,
        "custo_comp_nuvem_usd_por_gb": perf["nuvem"]["custo_comp_usd_por_gb"],
        "preco_onprem_derivado_usd_nucleo_h": derivado,
        "preco_onprem_no_json_usd_nucleo_h": ctx.preco_cpu["onprem"],
        "passou": bool(ok and abs(derivado - ctx.preco_cpu["onprem"]) < 0.0005),
    }


def fmt(x, n=2):
    if x is None:
        return "-"
    if isinstance(x, str):
        return x
    if abs(x) >= 1000:
        return "{:,.0f}".format(x).replace(",", ".")
    return ("{:." + str(n) + "f}").format(x).replace(".", ",")


def usd(x):
    if x is None:
        return "-"
    if abs(x) >= 1000:
        return "{:,.0f}".format(x).replace(",", ".")
    if abs(x) >= 1:
        return ("{:.2f}".format(x)).replace(".", ",")
    return ("{:.4f}".format(x)).replace(".", ",")


def tabela(cab, linhas):
    out = ["| " + " | ".join(cab) + " |", "|" + "|".join(["---"] * len(cab)) + "|"]
    for l in linhas:
        out.append("| " + " | ".join(str(c) for c in l) + " |")
    return "\n".join(out) + "\n"


def ordenar_classes(classes):
    return sorted(classes, key=lambda c: ORDEM_CLASSES.index(c) if c in ORDEM_CLASSES else 99)


def ordem_configs(cfgs):
    ordem = ["stored", "zlib-1", "zlib-6", "zlib-9", "zstd-1", "zstd-3", "zstd-3-long27", "zstd-9", "zstd-19", "zstd-22", "xz-1", "xz-6", "xz-9e", "bzip2-9"]
    return sorted(cfgs, key=lambda c: ordem.index(c) if c in ordem else 99)


def gerar_resumo(med, p, ctx, recs, grupos, liquido, otimo, otimo_restrito, marginal, recompressao, bloco, dic, ecore, vies, sens, teste, caminho):
    L = []
    L.append("# Resumo do benchmark Syntra (compressao sem perdas + economia)\n")
    meta = med["meta"]
    prot = med["protocolo"]
    L.append("Gerado por `economia.py` a partir de `medicoes.json` e `precos.json`. Data da execucao: %s. Unidades: MB = 10^6 B, GB = 10^9 B.\n" % prot["data_execucao"])
    L.append("## 1. Ambiente e protocolo\n")
    top = meta["topologia"]
    L.append(
        tabela(
            ["Item", "Valor"],
            [
                ["CPU", meta["cpu_modelo"]],
                ["P-cores (threads)", "%d nucleos fisicos, CPUs %s" % (top["P_nucleos_fisicos"], ",".join(map(str, top["P_threads"])))],
                ["E-cores (threads)", "%d nucleos fisicos, CPUs %s" % (top["E_nucleos_fisicos"], "%d-%d" % (min(top["E_threads"]), max(top["E_threads"])))],
                ["Governor / no_turbo", "%s / %s" % (meta["governor_cpu0"], meta["intel_no_turbo"])],
                ["Python / libzstd (modulo compression.zstd)", "%s / %s" % (meta["python"], meta["libzstd_python"])],
                ["zstd CLI / xz CLI", "%s / %s" % (meta["zstd_cli"], meta["xz_cli"])],
                ["zlib / bzip2", "%s / %s" % (meta["zlib_runtime"], meta["bzip2_cli"])],
                ["Repeticoes (mediana)", prot["repeticoes"]],
                ["Fatia maxima por arquivo", "%d MiB" % (prot["cap_bytes"] // (1 << 20))],
                ["P-cores da medicao principal (4 trabalhadores concorrentes)", ",".join(map(str, prot["cpus_principal"]))],
                ["Energia (RAPL)", med["energia"].get("motivo", "medida") if not med["energia"]["medida"] else "medida"],
                ["Violacoes de bit-exatidao", len(med.get("bit_exatidao_violada", []))],
                ["Celulas com falha de execucao", len(med.get("falhas", []))],
            ],
        )
    )
    L.append("\n### Arquivos usados (fatia medida, sha256 do arquivo inteiro)\n")
    L.append(
        tabela(
            ["Arquivo", "Classe", "Sintetico", "Tamanho", "Fatia", "sha256 (arquivo)"],
            [[a["nome"], a["classe"], "SIM" if a["sintetico"] else "nao", a["tamanho_bytes"], a["tamanho_fatia_bytes"], a["sha256_arquivo"][:16] + "..."] for a in med["arquivos"]],
        )
    )
    L.append("\nSha256 completos em `medicoes.json` (campo `arquivos`).\n")

    L.append("## 2. Requisito\n")
    L.append(
        "5 PB/mes: media rho = %s MB/s (%s GB/s); pico k=%s: %s MB/s (%s GB/s). Retencao T=%d meses; fator de vazao servidor/notebook = %s.\n"
        % (fmt(ctx.rho_mbps, 1), fmt(ctx.rho_mbps / 1000, 3), ctx.k, fmt(ctx.rho_pico_mbps, 1), fmt(ctx.rho_pico_mbps / 1000, 2), ctx.T, fmt(ctx.fator, 2))
    )
    L.append("Teste de validacao a mao: " + ("PASSOU" if teste["passou"] else "FALHOU") + " (detalhes em `economia.json`, campo `autoteste`).\n")

    L.append("## 3. Escala multi-nucleo (processos independentes, dados distintos, blocos de 1 MiB)\n")
    if "escala" in med:
        L.append("Ordem de fixacao: P-cores fisicos (CPUs 0,2,...,14), depois E-cores (16..31), depois irmaos SMT dos P-cores (1,3,...,15). Eficiencia = agregado / soma das vazoes isoladas de 1 nucleo (P-core, CPU 0) sobre os mesmos dados.\n")
        for cfg, v in med["escala"]["configs"].items():
            L.append("\n**%s** (duracao %.1f s por rodada, mediana de %d rodadas)\n" % (cfg, med["escala"]["duracao_s"], prot["repeticoes"]))
            L.append(
                tabela(
                    ["N", "comp agregada MB/s", "comp por nucleo MB/s", "comp eficiencia", "decomp agregada MB/s", "decomp eficiencia", "bit-exato"],
                    [
                        [
                            l["n"],
                            fmt(l["comp"]["agregado_MBps_mediana"], 0),
                            fmt(l["comp"]["por_nucleo_MBps_mediana"], 0),
                            fmt(l["comp"]["eficiencia"], 2),
                            fmt(l["decomp"]["agregado_MBps_mediana"], 0),
                            fmt(l["decomp"]["eficiencia"], 2),
                            "sim" if l["bit_exato"] else "NAO",
                        ]
                        for l in v["por_n"]
                    ],
                )
            )
    modo_p = val(p, "modo_principal")

    L.append("## 4. Razao (x) por classe e configuracao\n")
    for modo in ["inteiro", "bloco_1MiB", "bloco_64KiB"]:
        classes = ordenar_classes({c for (c, m) in grupos if m == modo})
        cfgs = ordem_configs({r["config"] for (c, m), l in grupos.items() if m == modo for r in l})
        linhas = []
        for c in classes:
            por = {r["config"]: r for r in grupos[(c, modo)]}
            linhas.append([c] + [fmt(por[g]["razao_x"], 2) if g in por else "-" for g in cfgs])
        L.append("\n**Modo %s** (razao = original / comprimido)\n" % modo)
        L.append(tabela(["classe"] + cfgs, linhas))

    L.append("\n## 5. Vazao por nucleo (MB/s, mediana) compressao / descompressao\n")
    for modo in ["inteiro", "bloco_1MiB"]:
        classes = ordenar_classes({c for (c, m) in grupos if m == modo})
        cfgs = ordem_configs({r["config"] for (c, m), l in grupos.items() if m == modo for r in l})
        linhas = []
        for c in classes:
            por = {r["config"]: r for r in grupos[(c, modo)]}
            linhas.append([c] + [("%s / %s" % (fmt(por[g]["theta_c_MBps"], 0), fmt(por[g]["theta_d_MBps"], 0))) if g in por else "-" for g in cfgs])
        L.append("\n**Modo %s**\n" % modo)
        L.append(tabela(["classe"] + cfgs, linhas))

    L.append("\n## 6. Penalidade de bloco (aumento do tamanho comprimido vs arquivo inteiro, %)\n")
    for modo in ["bloco_1MiB", "bloco_64KiB"]:
        cfgs = ordem_configs({b["config"] for b in bloco if b["modo"] == modo})
        classes = ordenar_classes({b["classe"] for b in bloco if b["modo"] == modo})
        linhas = []
        for c in classes:
            por = {b["config"]: b for b in bloco if b["modo"] == modo and b["classe"] == c}
            linhas.append([c] + [fmt(por[g]["aumento_tamanho_pct"], 1) if g in por else "-" for g in cfgs])
        L.append("\n**%s**\n" % modo)
        L.append(tabela(["classe"] + cfgs, linhas))

    L.append("\n## 7. Dicionario treinado (zstd, blocos de 64 KiB, dicionario ~110 KB, treino disjunto do teste)\n")
    if dic:
        L.append(
            tabela(
                ["classe", "config", "arquivos", "razao sem", "razao com", "ganho de razao %", "reducao de tamanho %", "comp MB/s sem->com", "decomp MB/s sem->com", "bit-exato"],
                [
                    [
                        d["classe"],
                        d["config"],
                        ",".join(d["arquivos"]),
                        fmt(d["razao_x_sem"], 2),
                        fmt(d["razao_x_com"], 2),
                        fmt(d["ganho_razao_pct"], 1),
                        fmt(d["reducao_tamanho_pct"], 1),
                        "%s -> %s" % (fmt(d["theta_c_sem_MBps"], 0), fmt(d["theta_c_com_MBps"], 0)),
                        "%s -> %s" % (fmt(d["theta_d_sem_MBps"], 0), fmt(d["theta_d_com_MBps"], 0)),
                        "sim" if d["bit_exato"] else "NAO",
                    ]
                    for d in dic
                ],
            )
        )
        L.append("\nDelta de valor liquido por GB (dicionario - sem dicionario), US$/GB, cenario nuvem+s3_standard, camadas quente/morno/frio (nao inclui custo de treino, versionamento e armazenamento de dicionarios):\n")
        L.append(
            tabela(
                ["classe", "config", "quente", "morno", "frio"],
                [[d["classe"], d["config"]] + [usd(d["delta_liquido_usd_gb"]["nuvem+s3_standard|" + c]) for c in CAMADAS] for d in dic],
            )
        )

    L.append("\n## 8. Requisito 5 PB/mes: nucleos e custo (modo %s)\n" % modo_p)
    L.append("N medio e N pico incluem a eficiencia de escala medida (zstd-3, interpolada, constante acima de N=32). Custo mensal = N medio x preco x 730 h. `cabe64` = N pico <= 64.\n")
    for c in ordenar_classes({c for (c, m) in grupos if m == modo_p}):
        lista = sorted(grupos[(c, modo_p)], key=lambda r: r["fracao_economizada"])
        L.append("\n**%s**\n" % c)
        L.append(
            tabela(
                ["config", "razao x", "economia %", "comp MB/s", "decomp MB/s", "N medio", "N pico", "cabe64", "custo/mes nuvem US$", "custo/mes on-prem US$", "Pareto"],
                [
                    [
                        r["config"],
                        fmt(r["razao_x"], 2),
                        fmt(100 * r["fracao_economizada"], 1),
                        fmt(r["theta_c_MBps"], 0),
                        fmt(r["theta_d_MBps"], 0),
                        fmt(r["perfil"]["N_medio"], 1),
                        fmt(r["perfil"]["N_pico"], 1),
                        "sim" if r["perfil"]["cabe_servidor_64"] else "nao",
                        usd(r["perfil"]["nuvem"]["custo_mensal_usd"]),
                        usd(r["perfil"]["onprem"]["custo_mensal_usd"]),
                        "*" if r["pareto_razao_x_vazao_comp"] else "",
                    ]
                    for r in lista
                ],
            )
        )
    L.append("\nSensibilidade do N pico (classe mix_real, modo %s) a curva de eficiencia de escala e ao fator de vazao do nucleo de servidor:\n" % modo_p)
    rotulos = []
    for x in sens:
        if x["cenario_escala"] not in rotulos:
            rotulos.append(x["cenario_escala"])
    linhas = []
    for cfg in ("zstd-1", "zstd-3", "zstd-9", "zstd-19", "xz-9e"):
        linha = [cfg]
        for rot in rotulos:
            x = next((x for x in sens if x["classe"] == "mix_real" and x["config"] == cfg and x["cenario_escala"] == rot), None)
            linha.append("-" if x is None else "%s (ef %s)%s" % (fmt(x["N_pico"], 1), fmt(x["eficiencia_comp_no_pico"], 2), "" if x["cabe_servidor_64"] else " *>64"))
        linhas.append(linha)
    L.append(tabela(["config"] + rotulos, linhas))
    L.append("\nConfiguracoes que cumprem 5 PB/mes com <= 64 nucleos no pico (modo %s):\n" % modo_p)
    linhas = []
    for c in ordenar_classes({c for (c, m) in grupos if m == modo_p}):
        ok = [r["config"] for r in sorted(grupos[(c, modo_p)], key=lambda r: r["fracao_economizada"]) if r["perfil"]["cabe_servidor_64"]]
        linhas.append([c, ", ".join(ok) if ok else "nenhuma"])
    L.append(tabela(["classe", "configuracoes (N pico <= 64)"], linhas))

    L.append("\n## 9. Configuracao otima por classe, camada e cenario (maior valor liquido; modo %s)\n" % modo_p)
    L.append("Valor liquido por GB = economia de armazenamento (T=%d meses) - custo de compressao - n x custo de descompressao. Tabela com cenario nuvem+s3_standard; demais cenarios na tabela seguinte.\n" % ctx.T)
    linhas = []
    for c in ordenar_classes({c for (c, m) in grupos if m == modo_p}):
        for camada in CAMADAS:
            o = next(x for x in otimo if x["classe"] == c and x["modo"] == modo_p and x["cenario"] == "nuvem+s3_standard" and x["camada"] == camada)
            r = next(r for r in grupos[(c, modo_p)] if r["config"] == o["config"])
            linhas.append(
                [
                    c,
                    camada,
                    o["config"],
                    fmt(o["razao_x"], 2),
                    fmt(o["theta_c_MBps"], 0),
                    fmt(o["theta_d_MBps"], 0),
                    fmt(o["N_pico"], 1),
                    usd(o["custo_mensal_cpu_usd"]),
                    usd(o["liquido_usd_gb"]),
                    usd(o["liquido_mensal_coorte_usd"]),
                ]
            )
    L.append(tabela(["classe", "camada", "config otima", "razao x", "comp MB/s", "decomp MB/s", "N pico", "custo CPU/mes nuvem US$", "liquido US$/GB", "liquido US$/coorte mensal"], linhas))
    L.append("\nOtimo por cenario (quente / morno / frio):\n")
    linhas = []
    for c in ordenar_classes({c for (c, m) in grupos if m == modo_p}):
        linha = [c]
        for cen in CENARIOS_DESTAQUE:
            linha.append(" / ".join(next(x for x in otimo if x["classe"] == c and x["modo"] == modo_p and x["cenario"] == cen and x["camada"] == camada)["config"] for camada in CAMADAS))
        linhas.append(linha)
    L.append(tabela(["classe"] + CENARIOS_DESTAQUE, linhas))
    L.append("\nOtimo com restricao do requisito (N pico <= 64 nucleos), cenario nuvem+s3_standard, modo %s:\n" % modo_p)
    linhas = []
    for c in ordenar_classes({c for (c, m) in grupos if m == modo_p}):
        for camada in CAMADAS:
            o = next(x for x in otimo_restrito if x["classe"] == c and x["modo"] == modo_p and x["cenario"] == "nuvem+s3_standard" and x["camada"] == camada)
            linhas.append([c, camada, o["config"], fmt(o["razao_x"], 2), fmt(o["theta_c_MBps"], 0), fmt(o["theta_d_MBps"], 0), fmt(o["N_pico"], 1), usd(o["custo_mensal_cpu_usd"]), usd(o["liquido_usd_gb"]), usd(o["perda_vs_otimo_irrestrito_usd_gb"])])
    L.append(tabela(["classe", "camada", "config otima (<=64)", "razao x", "comp MB/s", "decomp MB/s", "N pico", "custo CPU/mes nuvem US$", "liquido US$/GB", "perda vs irrestrito US$/GB"], linhas))
    L.append("\nOtimo restrito por cenario (quente / morno / frio):\n")
    linhas = []
    for c in ordenar_classes({c for (c, m) in grupos if m == modo_p}):
        linha = [c]
        for cen in CENARIOS_DESTAQUE:
            linha.append(" / ".join(next(x for x in otimo_restrito if x["classe"] == c and x["modo"] == modo_p and x["cenario"] == cen and x["camada"] == camada)["config"] for camada in CAMADAS))
        linhas.append(linha)
    L.append(tabela(["classe"] + CENARIOS_DESTAQUE, linhas))
    L.append("\nOtimo no modo inteiro (arquivo inteiro; referencia superior, sem acesso aleatorio), cenario nuvem+s3_standard:\n")
    linhas = []
    for c in ordenar_classes({c for (c, m) in grupos if m == "inteiro"}):
        linhas.append([c] + [next(x for x in otimo if x["classe"] == c and x["modo"] == "inteiro" and x["cenario"] == "nuvem+s3_standard" and x["camada"] == camada)["config"] for camada in CAMADAS])
    L.append(tabela(["classe", "quente", "morno", "frio"], linhas))

    L.append("\n## 10. Analise marginal (modo %s, cenario nuvem+s3_standard, camada morno)\n" % modo_p)
    L.append("Cadeia zstd-1 -> zstd-3 -> zstd-9 -> zstd-19 -> xz-9e (ordem de razao tipica; a ordenacao completa por razao de cada classe, todas as camadas e cenarios esta em `economia.json`, campo `marginal`). Ganho = economia de armazenamento adicional por GB; custo = CPU adicional por GB (compressao + n x descompressao); razao = ganho / custo.\n")
    cadeia = ["zstd-1", "zstd-3", "zstd-9", "zstd-19", "xz-9e"]
    indice = {}
    for l in liquido:
        if l["modo"] == modo_p and l["cenario"] == "nuvem+s3_standard" and l["camada"] == "morno":
            indice[(l["classe"], l["config"])] = l
    linhas = []
    for c in ordenar_classes({c for (c, m) in grupos if m == modo_p}):
        for a_, b_ in zip(cadeia, cadeia[1:]):
            la = indice.get((c, a_))
            lb = indice.get((c, b_))
            if la is None or lb is None:
                continue
            ra = next(r for r in grupos[(c, modo_p)] if r["config"] == a_)
            rb = next(r for r in grupos[(c, modo_p)] if r["config"] == b_)
            dv = lb["valor_armazenamento_usd_gb"] - la["valor_armazenamento_usd_gb"]
            dc = (lb["custo_comp_usd_gb"] + lb["n_leituras"] * lb["custo_decomp_por_leitura_usd_gb"]) - (la["custo_comp_usd_gb"] + la["n_leituras"] * la["custo_decomp_por_leitura_usd_gb"])
            linhas.append(
                [
                    c,
                    "%s -> %s" % (a_, b_),
                    fmt(100 * (max(rb["fracao_economizada"], 0) - max(ra["fracao_economizada"], 0)), 2),
                    usd(dv),
                    usd(dc),
                    fmt(dv / dc, 1) if dc > 0 else "-",
                    usd(dv - dc),
                    "sim" if dv - dc > 0 else "nao",
                ]
            )
    L.append(tabela(["classe", "passo", "delta economia (pp)", "ganho armazenamento US$/GB", "custo CPU US$/GB", "ganho/custo", "delta liquido US$/GB", "vale"], linhas))

    L.append("\n## 11. Regra de recompressao em camadas (modo %s)\n" % modo_p)
    L.append("Dado ja em zstd-1 ou zstd-3 na ingestao. Meses de vida restante minimos para o reempacotamento se pagar: M* = (custo de descompressao da origem + custo de compressao do destino) / (ganho mensal de armazenamento - taxa de leitura x aumento do custo de leitura). 'nunca' = o custo de leitura extra supera o ganho de armazenamento. CPU de reempacotamento com eficiencia de escala de servidor cheio (N=32).\n")
    linhas = []
    for r in recompressao:
        if r["modo"] != modo_p or r["cenario"] not in CENARIOS_DESTAQUE or r["origem"] != "zstd-3" or r["destino"] not in ("zstd-19", "xz-9e"):
            continue
        linhas.append(
            [
                r["classe"],
                r["cenario"],
                r["camada"],
                "%s -> %s" % (r["origem"], r["destino"]),
                fmt(100 * r["delta_fracao_economizada"], 1),
                usd(r["custo_repack_usd_gb"]),
                fmt(r["meses_equilibrio"], 2) if r["meses_equilibrio"] is not None else "nunca",
            ]
        )
    linhas.sort(key=lambda l: (ORDEM_CLASSES.index(l[0]) if l[0] in ORDEM_CLASSES else 99, l[1], l[3], CAMADAS.index(l[2])))
    L.append(tabela(["classe", "cenario", "camada", "reempacotar", "delta economia (pp)", "custo repack US$/GB", "M* (meses restantes)"], linhas))

    L.append("\n## 12. Memoria (pico de RSS, entrada de 32 MiB de %s)\n" % "mozilla")
    if med.get("memoria"):
        L.append(
            tabela(
                ["config", "RSS compressao MB", "RSS descompressao MB", "comprimido (bytes)"],
                [[m["config"], fmt(m["rss_comp_kb"] / 1000, 1), fmt(m["rss_decomp_kb"] / 1000, 1), m["comprimido_bytes"]] for m in med["memoria"]],
            )
        )
    L.append("\n## 13. P-core vs E-core (subconjunto dickens+mozilla, 1 nucleo isolado)\n")
    if ecore:
        L.append(
            tabela(
                ["modo", "config", "P comp", "E comp", "E/P comp", "P decomp", "E decomp", "E/P decomp"],
                [[e["modo"], e["config"], fmt(e["P_comp_MBps"], 0), fmt(e["E_comp_MBps"], 0), fmt(e["E_sobre_P_comp"], 2), fmt(e["P_decomp_MBps"], 0), fmt(e["E_decomp_MBps"], 0), fmt(e["E_sobre_P_decomp"], 2)] for e in ecore],
            )
        )
    L.append("\n### Vies de concorrencia da medicao principal (4 trabalhadores) vs P-core isolado, mesmas celulas\n")
    if vies:
        L.append(tabela(["modo", "config", "comp paralelo/isolado", "decomp paralelo/isolado"], [[v["modo"], v["config"], fmt(v["comp_paralelo_sobre_isolado"], 3), fmt(v["decomp_paralelo_sobre_isolado"], 3)] for v in vies]))
    L.append("\n## 14. Conferencia cruzada com `zstd -b` (CLI, mesma fatia de 8 MiB, P-core CPU 0)\n")
    if med.get("cli_referencia_zstd_bench"):
        iso = {}
        for x in med.get("ecore", []):
            if x["tipo_nucleo"] == "P_isolado":
                iso[(x["arquivo"], x["modo"], x["config"])] = x
        linhas = []
        for c in med["cli_referencia_zstd_bench"]:
            modo = "inteiro" if not c["bloco"] else ("bloco_1MiB" if c["bloco"] == 1048576 else "bloco_64KiB")
            h = iso.get((c["arquivo"], modo, "zstd-%d" % c["nivel"]))
            linhas.append(
                [
                    c["arquivo"],
                    c["nivel"],
                    modo,
                    fmt(c["comp_MBps"], 0),
                    fmt(c["decomp_MBps"], 0),
                    fmt(h["comp"]["MBps_mediana"], 0) if h else "-",
                    fmt(h["decomp"]["MBps_mediana"], 0) if h else "-",
                    fmt(h["comp"]["MBps_mediana"] / c["comp_MBps"], 2) if h else "-",
                    fmt(h["decomp"]["MBps_mediana"] / c["decomp_MBps"], 2) if h else "-",
                    "%d vs %d" % (c["bytes_comp"], h["bytes_comp"]) if h else "-",
                ]
            )
        L.append(tabela(["arquivo", "nivel", "modo", "CLI comp MB/s", "CLI decomp MB/s", "harness comp MB/s", "harness decomp MB/s", "comp harness/CLI", "decomp harness/CLI", "bytes comprimidos CLI vs harness"], linhas))
    L.append("\n## 15. Falhas e violacoes\n")
    L.append("Violacoes de bit-exatidao: %s\n" % (json.dumps(med.get("bit_exatidao_violada", [])) if med.get("bit_exatidao_violada") else "nenhuma (sha256 do original == sha256 do descomprimido em todas as medicoes)"))
    L.append("Celulas com falha: %s\n" % (json.dumps(med.get("falhas", [])[:5]) if med.get("falhas") else "nenhuma"))
    with open(caminho, "w") as f:
        f.write("\n".join(L))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--medicoes", default=None)
    ap.add_argument("--precos", default=None)
    ap.add_argument("--saida", default=None)
    ap.add_argument("--resumo", default=None)
    ap.add_argument("--autoteste", action="store_true")
    a = ap.parse_args()
    base = os.path.dirname(os.path.abspath(__file__))
    precos = a.precos or os.path.join(base, "precos.json")
    p = carregar(precos)
    teste = autoteste(p)
    if a.autoteste:
        print(json.dumps(teste, indent=1))
        sys.exit(0 if teste["passou"] else 1)
    med = carregar(a.medicoes or os.path.join(base, "resultados", "medicoes.json"))
    ctx, recs, grupos, liquido, otimo, marginal, recompressao, otimo_restrito = calcular(med, p)
    bloco = penalidade_bloco(recs)
    dic = dicionario_ganho(ctx, med)
    ecore, vies = ecore_e_vies(med)
    sens = sensibilidade(p, med, recs)
    saida = a.saida or os.path.join(base, "resultados", "economia.json")
    obj = {
        "autoteste": teste,
        "requisito": {
            "volume_mensal_bytes": ctx.volume_b,
            "rho_MBps_medio": ctx.rho_mbps,
            "rho_MBps_pico": ctx.rho_pico_mbps,
            "k_pico": ctx.k,
            "eficiencia_escala_usada": ctx.tab,
        },
        "parametros": p,
        "configuracoes": recs,
        "liquido": liquido,
        "otimo": otimo,
        "otimo_restrito_64_nucleos_no_pico": otimo_restrito,
        "marginal": marginal,
        "recompressao": recompressao,
        "penalidade_bloco": bloco,
        "dicionario": dic,
        "ecore_vs_pcore": ecore,
        "vies_concorrencia": vies,
        "sensibilidade_escala_e_fator_servidor": sens,
    }
    with open(saida, "w") as f:
        json.dump(obj, f, indent=1)
    resumo = a.resumo or os.path.join(base, "resultados", "resumo.md")
    gerar_resumo(med, p, ctx, recs, grupos, liquido, otimo, otimo_restrito, marginal, recompressao, bloco, dic, ecore, vies, sens, teste, resumo)
    print("autoteste:", "ok" if teste["passou"] else "FALHOU", "| configuracoes:", len(recs), "| linhas liquido:", len(liquido))


if __name__ == "__main__":
    main()
