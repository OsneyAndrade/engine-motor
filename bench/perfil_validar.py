import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import math
import os
import re
import shutil
import sys
import tempfile
import time
from collections import Counter, defaultdict

import numpy as np
from compression import zstd

sys.dont_write_bytecode = True

BASE = os.path.dirname(os.path.abspath(__file__))
MIB = 1 << 20


def carregar(nome, arquivo):
    spec = importlib.util.spec_from_file_location(nome, os.path.join(BASE, arquivo))
    m = importlib.util.module_from_spec(spec)
    sys.modules[nome] = m
    spec.loader.exec_module(m)
    return m


def medicoes(caminho):
    with open(caminho) as f:
        med = json.load(f)
    saida = {}
    for x in med["principal"]:
        saida[(x["arquivo"], x["modo"], x["config"])] = (x["bytes_orig"], x["bytes_comp"])
    for x in med["completo"]:
        saida[(x["arquivo"], "completo", x["config"])] = (x["bytes_orig"], x["bytes_comp"])
    return saida, {a["nome"]: a["tamanho_bytes"] for a in med["arquivos"]}, {a["nome"]: a["classe"] for a in med["arquivos"]}


def por_arquivo(est, tool):
    saida = {}
    for i, acc in est.por_arquivo_amostra.items():
        a = est.v.arquivos[i]
        nome = a.nome.decode("utf-8", "backslashreplace")
        r = {}
        r["zstd-1"] = (acc[0], acc[1])
        r["zstd-3"] = (acc[0], acc[2])
        r["zstd-9"] = (acc[0], acc[3])
        for codec, k in (("zstd-19", 5), ("xz-9e", 6), ("bzip2-9", 7)):
            r[codec] = (acc[0] / acc[2] * acc[9] / acc[k], 1.0) if acc[k] and acc[2] else (0, 0)
        saida[nome] = {"codecs": r, "classe": a.classe, "tamanho": a.tam, "unidades": acc[8]}
    return saida


def rodar_amostra(tool, raiz, fast, slow, semente, processos, reamostras):
    opc = tool.Opc(raiz=raiz, revelar_nomes=True, amostra_mib=fast, amostra_mib_lento=slow, semente=semente, processos=processos, reamostras=reamostras)
    est = tool.Estado(opc)
    try:
        amo = tool.etapa_amostrar(est)
        return amo, por_arquivo(est, tool), {a.nome.decode("utf-8", "backslashreplace"): a.classe for a in est.v.arquivos}
    finally:
        est.fechar()


def razao(par):
    return par[0] / par[1] if par and par[1] else None


def erro_rel(x, ref):
    if x is None or ref is None or ref == 0:
        return None
    return 100.0 * (x - ref) / ref


def estat(erros, pesos=None):
    v = [(abs(e), (pesos[i] if pesos else 1.0)) for i, e in enumerate(erros) if e is not None]
    if not v:
        return {"n": 0}
    a = np.array([x[0] for x in v])
    w = np.array([x[1] for x in v], dtype=float)
    return {"n": len(v), "erro_abs_medio_pct": float(a.mean()), "erro_abs_medio_ponderado_por_bytes_pct": float((a * w).sum() / w.sum()), "erro_abs_max_pct": float(a.max())}


def parte_a(tool, dados, med, tamanhos, rotulos_med, processos, sementes, log):
    t0 = time.time()
    log("A1: censo (todos os blocos de 1 MiB, todos os codecs)")
    amo_c, arq_c, cls_c = rodar_amostra(tool, dados, 10**6, 10**6, 1, processos, 200)
    log("A1 pronto em %.0fs" % (time.time() - t0))
    t0 = time.time()
    log("A2: orcamento padrao (256 MiB rapidos, 32 MiB lentos)")
    amo_d, arq_d, _ = rodar_amostra(tool, dados, 256, 32, 1, processos, 1000)
    log("A2 pronto em %.0fs" % (time.time() - t0))
    codecs = tool.CODECS
    linhas = []
    for nome, d in sorted(arq_c.items()):
        if (nome, "bloco_1MiB", "zstd-3") not in med:
            continue
        for codec in codecs:
            censo = razao(d["codecs"][codec])
            amostra = razao(arq_d[nome]["codecs"][codec]) if nome in arq_d else None
            bloco = razao(med.get((nome, "bloco_1MiB", codec)))
            inteiro = razao(med.get((nome, "inteiro", codec)))
            completo = razao(med.get((nome, "completo", codec)))
            tam = d["tamanho"]
            cobre_tudo = tam <= 8 * MIB
            ref_inteiro = inteiro if cobre_tudo else completo
            linhas.append(
                {
                    "arquivo": nome,
                    "classe_da_ferramenta": d["classe"],
                    "classe_medicoes": rotulos_med.get(nome),
                    "tamanho": tam,
                    "codec": codec,
                    "razao_censo_1MiB": censo,
                    "razao_amostra_padrao": amostra,
                    "razao_medicoes_bloco_1MiB_prefixo_8MiB": bloco,
                    "razao_medicoes_inteiro": inteiro,
                    "razao_medicoes_completo": completo,
                    "arquivo_coberto_pelo_prefixo_de_8MiB": cobre_tudo,
                    "referencia_inteira": "inteiro (8 MiB)" if cobre_tudo and inteiro else ("completo (arquivo todo, quadro unico)" if completo else None),
                    "erro_amostra_vs_censo_pct": erro_rel(amostra, censo),
                    "erro_censo_vs_medicoes_bloco_pct": erro_rel(censo, bloco) if cobre_tudo else None,
                    "erro_amostra_vs_medicoes_inteira_pct": erro_rel(amostra, ref_inteiro),
                    "erro_censo_vs_medicoes_inteira_pct": erro_rel(censo, ref_inteiro),
                }
            )
    resumo = {}
    for codec in codecs:
        L = [l for l in linhas if l["codec"] == codec]
        pesos = [l["tamanho"] for l in L]
        resumo[codec] = {
            "amostra_padrao_vs_censo": estat([l["erro_amostra_vs_censo_pct"] for l in L], pesos),
            "censo_vs_medicoes_bloco_arquivos_inteiros_no_prefixo": estat([l["erro_censo_vs_medicoes_bloco_pct"] for l in L], pesos),
            "amostra_padrao_vs_medicoes_inteira": estat([l["erro_amostra_vs_medicoes_inteira_pct"] for l in L], pesos),
            "censo_vs_medicoes_inteira": estat([l["erro_censo_vs_medicoes_inteira_pct"] for l in L], pesos),
        }
    log("A3: orcamento pequeno (16/4 MiB), %d sementes" % len(sementes))
    corridas = []
    cobertura = defaultdict(lambda: defaultdict(lambda: [0, 0]))
    erros_classe = defaultdict(lambda: defaultdict(list))
    erros_arq = defaultdict(list)
    t0 = time.time()
    for s in sementes:
        amo_s, arq_s, _ = rodar_amostra(tool, dados, 16, 4, s, processos, 400)
        corridas.append(s)
        for c, d in amo_s["por_classe"].items():
            ref = amo_c["por_classe"].get(c)
            if ref is None:
                continue
            for codec in codecs:
                v = ref["codecs"][codec]["economia"]
                e = d["codecs"][codec]
                if e["economia"] is None or v is None:
                    continue
                ic = e["ic95_economia"]
                dentro = ic[0] is not None and ic[0] - 1e-12 <= v <= ic[1] + 1e-12
                cobertura[c][codec][0] += 1 if dentro else 0
                cobertura[c][codec][1] += 1
                erros_classe[c][codec].append(100.0 * (e["economia"] - v))
        for nome, d in arq_s.items():
            if nome in arq_c:
                for codec in codecs:
                    erros_arq[codec].append(erro_rel(razao(d["codecs"][codec]), razao(arq_c[nome]["codecs"][codec])))
    log("A3 pronto em %.0fs" % (time.time() - t0))
    por_classe = {}
    for c in cobertura:
        ref = amo_c["por_classe"][c]
        por_classe[c] = {
            "arquivos": ref["arquivos_populacao"],
            "bytes": ref["bytes_populacao"],
            "baixa_confianca": ref["baixa_confianca"],
            "fracao_amostrada_orcamento_pequeno": None,
            "codecs": {
                codec: {
                    "economia_censo": ref["codecs"][codec]["economia"],
                    "erro_medio_pp": float(np.mean(erros_classe[c][codec])),
                    "erro_abs_medio_pp": float(np.mean(np.abs(erros_classe[c][codec]))),
                    "erro_abs_max_pp": float(np.max(np.abs(erros_classe[c][codec]))),
                    "ic95_cobriu_o_censo": cobertura[c][codec][0],
                    "de": cobertura[c][codec][1],
                }
                for codec in codecs
            },
        }
    cob_total = {codec: [sum(cobertura[c][codec][0] for c in cobertura if not amo_c["por_classe"][c]["baixa_confianca"]), sum(cobertura[c][codec][1] for c in cobertura if not amo_c["por_classe"][c]["baixa_confianca"])] for codec in codecs}
    cob_todas = {codec: [sum(cobertura[c][codec][0] for c in cobertura), sum(cobertura[c][codec][1] for c in cobertura)] for codec in codecs}
    arq_resumo = {codec: estat(erros_arq[codec]) for codec in codecs}
    return {
        "linhas_por_arquivo": linhas,
        "resumo_por_codec": resumo,
        "orcamento_pequeno": {"sementes": corridas, "por_classe": por_classe, "cobertura_ic95_classes_com_5_ou_mais_arquivos": cob_total, "cobertura_ic95_todas_as_classes": cob_todas, "erro_por_arquivo_vs_censo": arq_resumo},
        "censo_por_classe": {c: {"arquivos": d["arquivos_populacao"], "bytes": d["bytes_populacao"], "zstd-3": d["codecs"]["zstd-3"]["economia"], "xz-9e": d["codecs"]["xz-9e"]["economia"]} for c, d in amo_c["por_classe"].items()},
        "classes_ferramenta_vs_medicoes": {n: [cls_c[n], rotulos_med.get(n)] for n in sorted(cls_c) if n in rotulos_med},
    }


def sha_arquivo(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        while True:
            b = f.read(1 << 22)
            if not b:
                break
            h.update(b)
    return h.hexdigest()


def delta_exato(ant, nov):
    with open(ant, "rb") as f:
        ref = f.read()
    with open(nov, "rb") as f:
        novo = f.read()
    wl = min(27, max(21, math.ceil(math.log2(len(ref) + len(novo)))))
    d = zstd.ZstdDict(ref, is_raw=True)
    opc = {zstd.CompressionParameter.compression_level: 3, zstd.CompressionParameter.window_log: wl, zstd.CompressionParameter.enable_long_distance_matching: 1}
    c = zstd.compress(novo, options=opc, zstd_dict=d.as_prefix)
    return len(c), len(zstd.compress(novo, level=3)), len(novo)


def parte_b(tool, sint, dados, processos, resultados, log):
    base = tempfile.mkdtemp(prefix="perfil-validar-")
    try:
        log("B: construindo o dataset versionado sintetico")
        man = sint.construir(base, silesia=os.path.join(dados, "silesia"), canterbury=os.path.join(dados, "canterbury"), real=True)
        raiz = man["raiz"]
        bytes_v1 = sum(os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(os.path.join(raiz, "v1")) for f in fs if not os.path.islink(os.path.join(d, f)) and os.access(os.path.join(d, f), os.R_OK))
        saida = os.path.join(base, "saida")
        params, avisos = tool.carregar_params(None)
        opc = tool.Opc(raiz=raiz, saida=saida, processos=processos, semente=1)
        est = tool.Estado(opc)
        t0 = time.time()
        try:
            tool.preparar_saida(saida)
            log("B: rodando a ferramenta (padrao: 256/32 MiB, ate 200 pares)")
            var, amo, eco, perfil = tool.pipeline_tudo(est, params, avisos, saida)
            tempo = time.time() - t0
            log("B: pronto em %.0fs" % tempo)
            esperado = {os.fsencode(k): v for k, v in man["esperado_classe"].items()}
            ok = 0
            tot = 0
            erros = []
            por_cls = defaultdict(lambda: [0, 0])
            nomes_classe = {}
            for a in est.v.arquivos:
                rel = est.v.caminho(a.dir, a.nome)
                nomes_classe[rel] = a
                if rel in esperado:
                    tot += 1
                    por_cls[esperado[rel]][1] += 1
                    if a.classe == esperado[rel]:
                        ok += 1
                        por_cls[esperado[rel]][0] += 1
                    else:
                        erros.append([rel.decode("utf-8", "backslashreplace"), esperado[rel], a.classe])
            h = defaultdict(list)
            for d, ds, fs in os.walk(raiz):
                for f in fs:
                    p = os.path.join(d, f)
                    if os.path.islink(p) or not os.path.isfile(p) or not os.access(p, os.R_OK) or os.path.getsize(p) == 0:
                        continue
                    h[(os.path.getsize(p), sha_arquivo(p))].append(p)
            dup_oraculo = sum((len(v) - 1) * k[0] for k, v in h.items() if len(v) > 1)
            grupos_oraculo = sum(1 for v in h.values() if len(v) > 1)
            fam_pares = set()
            for chave, cadeia in est.familias:
                for ant, nov in zip(cadeia, cadeia[1:]):
                    fam_pares.add((est.v.caminho(ant.dir, ant.nome), est.v.caminho(nov.dir, nov.nome)))
            esperados = [(os.fsencode(a), os.fsencode(b)) for a, b in man["pares_versoes_modificadas"]]
            achados = [p for p in esperados if p in fam_pares]
            falsos = [p for p in fam_pares if p not in set(esperados)]
            medidos = {(r["ant"], r["nov"]): r for r in est.pares_medidos}
            idx = {est.v.caminho(a.dir, a.nome): a.i for a in est.v.arquivos}
            falsos_med = []
            for ant, nov in falsos:
                r = medidos.get((idx[ant], idx[nov]))
                if r:
                    falsos_med.append(r["patch"] / max(1, r["z3"]))
            exato = defaultdict(lambda: [0, 0, 0])
            for a, b in man["pares_versoes_modificadas"]:
                ia = idx.get(os.fsencode(a))
                ib = idx.get(os.fsencode(b))
                if ia is None or ib is None:
                    continue
                if os.path.getsize(os.path.join(raiz, b)) > 64 * MIB or os.path.getsize(os.path.join(raiz, a)) > 64 * MIB:
                    continue
                p, z, n = delta_exato(os.path.join(raiz, a), os.path.join(raiz, b))
                cl = est.v.arquivos[ib].classe
                exato[cl][0] += p
                exato[cl][1] += z
                exato[cl][2] += 1
            delta_cls = {}
            for c, d in var["familias"]["delta_por_classe"].items():
                if d is None:
                    continue
                e = exato.get(c)
                delta_cls[c] = {"razao_ferramenta": d["razao_patch_sobre_z3"], "ic95": d["ic95_razao_patch_sobre_z3"], "pares_medidos": d["pares_medidos"], "razao_exata_dos_pares_esperados": (e[0] / e[1]) if e and e[1] else None, "pares_exatos": e[2] if e else 0}
            tot_p = sum(v[0] for v in exato.values())
            tot_z = sum(v[1] for v in exato.values())
            resultado_b = {
                "dataset": {"arquivos_v1_v2_total": var["inventario"]["n_arquivos"], "bytes": var["inventario"]["bytes"], "bytes_v1_legiveis": bytes_v1, "tempo_da_ferramenta_s": tempo, "bytes_lidos_total": perfil["registro_leitura"]["bytes_lidos_total"], "fracao_lida_do_inventario": perfil["registro_leitura"]["fracao_lida_do_inventario"]},
                "classificacao": {"acertos": ok, "total_com_expectativa": tot, "acuracia": ok / tot if tot else None, "por_classe": {c: v for c, v in por_cls.items()}, "erros": erros},
                "dedup": {"bytes_duplicados_ferramenta": var["dedup"]["bytes_duplicados"], "bytes_duplicados_oraculo_sha256": dup_oraculo, "grupos_ferramenta": var["dedup"]["grupos_de_duplicatas"], "grupos_oraculo": grupos_oraculo, "iguais": var["dedup"]["bytes_duplicados"] == dup_oraculo and var["dedup"]["grupos_de_duplicatas"] == grupos_oraculo, "bytes_lidos_hash_completo": var["dedup"]["bytes_lidos_hash_completo"], "bytes_candidatos_por_tamanho": var["dedup"]["bytes_candidatos_por_tamanho"]},
                "familias": {"pares_esperados": len(esperados), "pares_achados": len(achados), "recall": len(achados) / len(esperados) if esperados else None, "pares_extras_na_ferramenta": len(falsos), "razao_patch_z3_dos_extras_media": float(np.mean(falsos_med)) if falsos_med else None, "familias": var["familias"]["n_familias"], "pct_bytes_em_familias": var["familias"]["pct_bytes_em_familias"], "pares_medidos": var["familias"]["pares_medidos"], "pares_possiveis": var["familias"]["pares_possiveis"], "falhas_de_roundtrip": var["familias"]["pares_com_falha_de_roundtrip"], "delta_por_classe_vs_exato": delta_cls, "delta_agregado_ferramenta": var["familias"]["delta_agregado_todas_as_classes"], "razao_exata_agregada_dos_pares_esperados": tot_p / tot_z if tot_z else None},
                "ignorados_por_motivo": var["inventario"]["ignorados_por_motivo"],
                "ilegiveis_por_motivo": var["inventario"]["ilegiveis_por_motivo"],
                "resumo_executivo": perfil["resumo_executivo"],
                "estimativa_total": eco["total"]["politica_recomendada_com_dedup_e_delta"]["liquido_usd_mes"],
            }
            for nome in ("perfil.json", "perfil.md"):
                shutil.copyfile(os.path.join(saida, nome), os.path.join(resultados, nome.replace("perfil", "perfil_exemplo")))
        finally:
            est.fechar()
        log("B: variante com 12 pares medidos (extrapolacao do delta)")
        opc2 = tool.Opc(raiz=raiz, processos=processos, semente=3, max_pares=12)
        est2 = tool.Estado(opc2)
        try:
            var2 = tool.etapa_varrer(est2)
        finally:
            est2.fechar()
        ag2 = var2["familias"]["delta_agregado_todas_as_classes"]
        resultado_b["familias"]["variante_12_pares"] = {"razao_ferramenta": ag2["razao_patch_sobre_z3"], "ic95": ag2["ic95_razao_patch_sobre_z3"], "pares_medidos": ag2["pares_medidos"], "razao_exata": resultado_b["familias"]["razao_exata_agregada_dos_pares_esperados"], "ic95_cobre_o_exato": ag2["ic95_razao_patch_sobre_z3"][0] <= (resultado_b["familias"]["razao_exata_agregada_dos_pares_esperados"] or 0) <= ag2["ic95_razao_patch_sobre_z3"][1]}
        return resultado_b
    finally:
        sint.destruir(base)


def f2(x, n=2):
    if x is None:
        return "-"
    return ("{:." + str(n) + "f}").format(x).replace(".", ",")


def md_validacao(v):
    M = []
    M.append("# Validacao do perfil de dataset\n")
    M.append("Gerado por `bench/perfil_validar.py` em %s (Python %s, zstd %s). Corpora publicos regenerados com `preparar_dados.py` (Silesia, Canterbury Large e sinteticos) e um dataset versionado sintetico. Nada deste arquivo vem de dado de cliente; nomes de arquivo aparecem porque os dados sao publicos ou sinteticos.\n" % (v["data"], v["python"], v["zstd"]))
    M.append("## Resultado do autoteste\n")
    M.append("%s\n" % v["autoteste"]["linha_final"])
    a = v["a_corpora_publicos"]
    M.append("## A. Corpora publicos: razao por amostra contra as medidas de `medicoes.json`\n")
    M.append("Tres referencias: (1) **censo**: a mesma ferramenta com todos os blocos de 1 MiB do arquivo inteiro (isola o erro de amostragem); (2) **medicoes, bloco de 1 MiB**: `principal` com `modo=bloco_1MiB`, so os primeiros 8 MiB de cada arquivo (iguala exatamente o censo nos arquivos de ate 8 MiB); (3) **medicoes, inteira**: `inteiro` (quadro unico) nos arquivos de ate 8 MiB e `completo` (arquivo todo, quadro unico) para zstd-1/3/9 nos maiores. Erro = (estimativa - referencia) / referencia da razao, em %.\n")
    linhas = []
    for codec, r in a["resumo_por_codec"].items():
        x1 = r["amostra_padrao_vs_censo"]
        x2 = r["censo_vs_medicoes_bloco_arquivos_inteiros_no_prefixo"]
        x3 = r["amostra_padrao_vs_medicoes_inteira"]
        x4 = r["censo_vs_medicoes_inteira"]
        linhas.append([codec, "%s / %s / %s" % (f2(x1.get("erro_abs_medio_pct"), 3), f2(x1.get("erro_abs_max_pct"), 3), x1["n"]), "%s / %s / %s" % (f2(x2.get("erro_abs_medio_pct"), 3), f2(x2.get("erro_abs_max_pct"), 3), x2["n"]), "%s / %s / %s" % (f2(x3.get("erro_abs_medio_pct")), f2(x3.get("erro_abs_max_pct")), x3["n"]), "%s / %s / %s" % (f2(x4.get("erro_abs_medio_pct")), f2(x4.get("erro_abs_max_pct")), x4["n"])])
    M.append(tabela(["Codec", "orcamento padrao vs censo (medio / max %, n)", "censo vs medicoes bloco 1 MiB, arquivos <= 8 MiB (medio / max %, n)", "orcamento padrao vs medicoes inteira (medio / max %, n)", "censo vs medicoes inteira (medio / max %, n)"], linhas))
    M.append("\nO orcamento padrao (256 MiB por classe para zstd-1/3/9) e censo em todo o corpus; so zstd-19, xz-9e e bzip2-9 (32 MiB) amostram. A diferenca entre o censo e a medida inteira (quadro unico) e a penalidade de bloco de 1 MiB que o motor paga, nao erro de amostragem: ela e a mesma ordem de grandeza da tabela de penalidade de bloco do README.\n")
    M.append("### Por arquivo (zstd-3 e xz-9e)\n")
    linhas = []
    for l in a["linhas_por_arquivo"]:
        if l["codec"] not in ("zstd-3", "xz-9e"):
            continue
        linhas.append([l["arquivo"], l["classe_da_ferramenta"], l["classe_medicoes"], l["codec"], f2(l["razao_amostra_padrao"], 3), f2(l["razao_censo_1MiB"], 3), f2(l["razao_medicoes_bloco_1MiB_prefixo_8MiB"], 3), f2(l["razao_medicoes_inteiro"], 3) if l["arquivo_coberto_pelo_prefixo_de_8MiB"] else (f2(l["razao_medicoes_completo"], 3) if l["razao_medicoes_completo"] else "n/d"), f2(l["erro_amostra_vs_censo_pct"], 3), f2(l["erro_amostra_vs_medicoes_inteira_pct"])])
    M.append(tabela(["Arquivo", "Classe (ferramenta)", "Classe (medicoes)", "Codec", "Razao amostra", "Razao censo", "Razao medicoes bloco (8 MiB)", "Razao medicoes inteira", "Erro vs censo %", "Erro vs inteira %"], linhas))
    o = a["orcamento_pequeno"]
    M.append("\n### Orcamento pequeno (16 MiB rapidos, 4 MiB lentos por classe), %d sementes\n" % len(o["sementes"]))
    M.append("Erro por arquivo contra o censo (so blocos sorteados de cada arquivo): " + "; ".join("%s medio %s%% max %s%%" % (c, f2(x.get("erro_abs_medio_pct")), f2(x.get("erro_abs_max_pct"))) for c, x in o["erro_por_arquivo_vs_censo"].items()) + ".\n")
    M.append("Cobertura do IC95 (a economia do censo cai dentro do intervalo): classes com 5 ou mais arquivos: " + "; ".join("%s %d/%d" % (c, x[0], x[1]) for c, x in o["cobertura_ic95_classes_com_5_ou_mais_arquivos"].items()) + ". Todas as classes (inclui as de baixa confianca, com 1 a 4 arquivos): " + "; ".join("%s %d/%d" % (c, x[0], x[1]) for c, x in o["cobertura_ic95_todas_as_classes"].items()) + ".\n")
    linhas = []
    for c, d in sorted(o["por_classe"].items(), key=lambda kv: -kv[1]["bytes"]):
        z = d["codecs"]["zstd-3"]
        x = d["codecs"]["xz-9e"]
        linhas.append([c, d["arquivos"], "BAIXA" if d["baixa_confianca"] else "ok", f2(100 * z["economia_censo"], 1), f2(z["erro_abs_medio_pp"], 2), f2(z["erro_abs_max_pp"], 2), "%d/%d" % (z["ic95_cobriu_o_censo"], z["de"]), f2(100 * x["economia_censo"], 1), f2(x["erro_abs_medio_pp"], 2), f2(x["erro_abs_max_pp"], 2), "%d/%d" % (x["ic95_cobriu_o_censo"], x["de"])])
    M.append(tabela(["Classe (ferramenta)", "Arquivos", "Confianca", "zstd-3 economia censo %", "erro abs medio pp", "max pp", "IC95 cobriu", "xz-9e economia censo %", "erro abs medio pp", "max pp", "IC95 cobriu"], linhas))
    M.append("\n### Classe da ferramenta contra o rotulo das medicoes\n")
    M.append(tabela(["Arquivo", "Ferramenta", "Medicoes"], [[n, x[0], x[1]] for n, x in a["classes_ferramenta_vs_medicoes"].items()]))
    b = v["b_dataset_versionado"]
    M.append("\n## B. Dataset versionado sintetico (v1/v2 de uma arvore)\n")
    d = b["dataset"]
    M.append("%s arquivos, %.1f MiB; a ferramenta leu %.1f MiB (%s do inventario) em %.0f s.\n" % (d["arquivos_v1_v2_total"], d["bytes"] / MIB, d["bytes_lidos_total"] / MIB, f2(100 * d["fracao_lida_do_inventario"], 1) + "%", d["tempo_da_ferramenta_s"]))
    c = b["classificacao"]
    M.append("**Classificacao**: %d de %d arquivos com expectativa classificados como esperado (%s%%). Erros: %s.\n" % (c["acertos"], c["total_com_expectativa"], f2(100 * c["acuracia"], 1), "; ".join("%s esperado %s obtido %s" % (e[0], e[1], e[2]) for e in c["erros"]) or "nenhum"))
    dd = b["dedup"]
    M.append("**Dedup**: ferramenta %d bytes em %d grupos; oraculo (sha256 de todos os arquivos) %d bytes em %d grupos; iguais: %s. Bytes lidos no hash completo: %d de %d candidatos por tamanho.\n" % (dd["bytes_duplicados_ferramenta"], dd["grupos_ferramenta"], dd["bytes_duplicados_oraculo_sha256"], dd["grupos_oraculo"], "sim" if dd["iguais"] else "NAO", dd["bytes_lidos_hash_completo"], dd["bytes_candidatos_por_tamanho"]))
    fm = b["familias"]
    M.append("**Familias**: %d de %d pares v1->v2 modificados foram achados (recall %s%%); %d pares extras na ferramenta (razao patch/zstd-3 media %s: %s). %d familias, %s%% dos bytes; pares medidos %d de %d possiveis; falhas de roundtrip %d.\n" % (fm["pares_achados"], fm["pares_esperados"], f2(100 * fm["recall"], 1), fm["pares_extras_na_ferramenta"], f2(fm["razao_patch_z3_dos_extras_media"], 3), "extras sao pares de arquivos sem relacao" if (fm["razao_patch_z3_dos_extras_media"] or 0) > 0.8 else "extras tem alguma semelhanca", fm["familias"], f2(100 * fm["pct_bytes_em_familias"], 1), fm["pares_medidos"], fm["pares_possiveis"], fm["falhas_de_roundtrip"]))
    linhas = [[cl, f2(x["razao_ferramenta"], 4), "[%s; %s]" % (f2(x["ic95"][0], 4), f2(x["ic95"][1], 4)), x["pares_medidos"], f2(x["razao_exata_dos_pares_esperados"], 4), x["pares_exatos"]] for cl, x in fm["delta_por_classe_vs_exato"].items()]
    M.append(tabela(["Classe", "patch/zstd-3 (ferramenta)", "IC95", "Pares medidos", "patch/zstd-3 exato dos pares v1->v2 (arquivo inteiro)", "Pares exatos"], linhas))
    v12 = fm["variante_12_pares"]
    M.append("\nVariante com so 12 pares medidos (extrapolacao): ferramenta %s [%s; %s] contra exato %s; o IC95 cobre o exato: %s.\n" % (f2(v12["razao_ferramenta"], 4), f2(v12["ic95"][0], 4), f2(v12["ic95"][1], 4), f2(v12["razao_exata"], 4), "sim" if v12["ic95_cobre_o_exato"] else "NAO"))
    M.append("Ignorados: %s. Ilegiveis: %s.\n" % (b["ignorados_por_motivo"], b["ilegiveis_por_motivo"]))
    M.append("Resumo executivo gerado (anonimizado, tambem em `perfil_exemplo.md`):\n")
    for k, l in enumerate(b["resumo_executivo"], 1):
        M.append("%d. %s" % (k, l))
    M.append("")
    return "\n".join(M)


def tabela(cab, linhas):
    out = ["| " + " | ".join(cab) + " |", "|" + "|".join(["---"] * len(cab)) + "|"]
    for l in linhas:
        out.append("| " + " | ".join(str(c) for c in l) + " |")
    return "\n".join(out) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dados", required=True)
    ap.add_argument("--resultados", required=True)
    ap.add_argument("--processos", type=int, default=5)
    ap.add_argument("--sementes", type=int, default=8)
    ap.add_argument("--sem-autoteste", action="store_true")
    a = ap.parse_args()
    tool = carregar("perfil_dataset", "perfil_dataset.py")
    sint = carregar("perfil_sintetico", "perfil_sintetico.py")
    t_ini = time.time()

    def log(m):
        print("[%.0fs] %s" % (time.time() - t_ini, m), file=sys.stderr, flush=True)

    med, tamanhos, rotulos = medicoes(os.path.join(BASE, "resultados", "medicoes.json"))
    os.makedirs(a.resultados, exist_ok=True)
    parte = parte_a(tool, os.path.abspath(a.dados), med, tamanhos, rotulos, a.processos, list(range(1, a.sementes + 1)), log)
    b = parte_b(tool, sint, os.path.abspath(a.dados), a.processos, a.resultados, log)
    if a.sem_autoteste:
        auto = {"linha_final": "(autoteste nao executado nesta rodada)"}
    else:
        log("autoteste")
        mt = tool.carregar_modulo("perfil_testes", os.path.join(BASE, "perfil_testes.py"))
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            rc = mt.executar(tool)
        linhas = buf.getvalue().strip().splitlines()
        auto = {"rc": rc, "linha_final": linhas[-1], "saida": linhas}
    v = {"data": time.strftime("%Y-%m-%d %H:%M:%S"), "python": sys.version.split()[0], "zstd": zstd.zstd_version, "autoteste": auto, "a_corpora_publicos": parte, "b_dataset_versionado": b}
    with open(os.path.join(a.resultados, "perfil_validacao.json"), "w") as f:
        json.dump(tool.limpar(v), f, indent=1, ensure_ascii=True, allow_nan=False)
    with open(os.path.join(a.resultados, "perfil_validacao.md"), "w", encoding="utf-8") as f:
        f.write(md_validacao(tool.limpar(v)))
    log("validacao concluida")
    return 0


if __name__ == "__main__":
    sys.exit(main())
