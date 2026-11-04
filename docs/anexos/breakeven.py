import json
import sys


def margem_por_gb(p):
    ganho_storage = p["delta_savings"] * p["p_store_gb_mes"] * p["redundancia"] * p["meses"]
    c_enc = p["gpu_usd_h"] / p["thr_enc_gb_h"]
    c_dec = p["gpu_usd_h"] / p["thr_dec_gb_h"]
    custo_leitura = p["leituras_vida"] * c_dec
    c_base_enc = p["cpu_usd_h"] / p["thr_base_gb_h"]
    return {
        "ganho_storage": ganho_storage,
        "c_enc": c_enc,
        "c_dec": c_dec,
        "custo_leitura": custo_leitura,
        "c_base_enc": c_base_enc,
        "margem": ganho_storage - (c_enc - c_base_enc) - custo_leitura,
    }


def break_even_gb(p, m):
    fixo = p["treino_usd"] + p["pesquisa_usd"]
    modelo_store = p["modelo_gb"] * p["p_store_gb_mes"] * p["redundancia"] * p["meses"]
    fixo += modelo_store
    if m["margem"] <= 0:
        return None
    return fixo / m["margem"]


def rodar(cenarios):
    saida = {}
    for nome, p in cenarios.items():
        m = margem_por_gb(p)
        d = break_even_gb(p, m)
        saida[nome] = {
            **{k: round(v, 6) for k, v in m.items()},
            "break_even_gb": None if d is None else round(d, 1),
            "break_even_tb": None if d is None else round(d / 1000, 2),
        }
    return saida


if __name__ == "__main__":
    cenarios = json.load(open(sys.argv[1]))
    print(json.dumps(rodar(cenarios), indent=2, ensure_ascii=False))
