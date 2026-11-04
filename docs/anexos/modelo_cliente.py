P_GB = 5e6
T = 60
S = 0.611
COMP_MBS = 378.0
DEC_MBS = 1611.0
CPU_H = 0.0446
ARMAZ = {"S3 Standard": (0.023, 0.0446), "B2": (0.00695, 0.0446), "on-prem (inferido)": (0.002, 0.0135)}


def custo_cpu_gb(mbs, cpu_h=CPU_H):
    return cpu_h / (mbs * 3.6)


def linha(ret, p, c_move, m, n_dec, cpu_h=CPU_H):
    r_gb = ret * P_GB
    arm_coorte = S * r_gb * p * T
    arm_mes_estavel = S * r_gb * T * p
    mov_mes = S * P_GB * c_move * m
    cpu_mes = P_GB * custo_cpu_gb(COMP_MBS, cpu_h) + P_GB * n_dec * custo_cpu_gb(DEC_MBS, cpu_h)
    return arm_coorte, arm_mes_estavel, mov_mes, cpu_mes


print("custo de CPU por GB: compressao", round(custo_cpu_gb(COMP_MBS), 7), "descompressao", round(custo_cpu_gb(DEC_MBS), 7))
print()
print("A) economia de armazenamento (zstd-3, mix real, 61,1%), retencao 60 meses, ret = fracao retida do que processa")
for ret in (0.80, 0.90, 0.95):
    for nome, (p, cpu_h) in ARMAZ.items():
        c, e, _, _ = linha(ret, p, 0, 1, 1, cpu_h)
        print(f"  ret={ret:.2f} {nome:20s} por coorte mensal: US$ {c:>12,.0f}   regime estavel: US$ {e:>12,.0f}/mes")
print()
print("B) economia de movimento de dados por mes (S x 5 PB x c_move x passagens)")
for c_move in (0.0, 0.01, 0.05, 0.09):
    for m in (1, 3):
        _, _, mov, _ = linha(0.9, 0.023, c_move, m, 1)
        print(f"  c_move=US$ {c_move:.2f}/GB, {m} passagem(ns): US$ {mov:>10,.0f}/mes")
print()
print("C) custo de CPU da compressao + descompressao (nuvem vCPU 0,0446/h, eficiencia 1,0), por mes")
for n_dec in (1, 3, 10):
    _, _, _, cpu = linha(0.9, 0.023, 0, 1, n_dec)
    print(f"  {n_dec} leitura(s) por GB processado: US$ {cpu:>8,.0f}/mes")
print()
print("D) razao entre economia de armazenamento (regime estavel, ret 0,90) e CPU mensal (n_dec = 3)")
for nome, (p, cpu_h) in ARMAZ.items():
    _, e, _, cpu = linha(0.9, p, 0, 1, 3, cpu_h)
    print(f"  {nome:20s}: {e/cpu:,.0f}x")
