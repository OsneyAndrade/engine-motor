T = 60
GPU_L4 = 0.805
GPU_H100_LAMBDA = 4.29
CPU_VCPU_H = 0.0446

armazenamento = {
    "S3 Standard": 0.023,
    "R2 Standard": 0.015,
    "B2": 0.00695,
    "S3 Glacier IR": 0.004,
    "On-prem HDD all-in (inferência própria)": 0.002,
}

def margem(delta_s, p_gb_mes, thr_mb_s, gpu_h, leituras, meses=T):
    thr_gb_h = thr_mb_s * 3.6
    c = gpu_h / thr_gb_h
    ganho = delta_s * p_gb_mes * meses
    return ganho - c - leituras * c, c

print("custo de processar 1 GB (US$) por throughput, GPU L4 on-demand")
for thr in (0.01, 0.1, 1, 10, 100):
    print(f"  {thr:>6} MB/s -> US$ {GPU_L4/(thr*3.6):.4f}/GB   energia 72W: {72/(thr*3.6)/1000*1000:.1f} Wh/GB")

print()
print("throughput mínimo (MB/s) para margem > 0, sem leituras (n=0), T=60 meses")
for nome, p in armazenamento.items():
    linha = []
    for ds in (0.02, 0.08, 0.20, 0.40):
        ganho = ds * p * T
        thr_min = GPU_L4 / (ganho * 3.6)
        linha.append(f"ΔS={ds:.2f}: {thr_min:8.2f}")
    print(f"  {nome:42s} " + " | ".join(linha))

print()
print("margem por GB (US$) com 1 MB/s em L4, por n leituras e ΔS=0.20")
for nome, p in armazenamento.items():
    linha = []
    for n in (0, 1, 10, 100):
        m, _ = margem(0.20, p, 1, GPU_L4, n)
        linha.append(f"n={n}: {m:7.3f}")
    print(f"  {nome:42s} " + " | ".join(linha))
