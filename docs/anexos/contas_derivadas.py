pue = 1.52
print("== energia: processar vs economizar (por GB de dado bruto), vida 5 anos")
for nome, kwh_tb_ano in (("HDD idle 1.89", 1.89), ("HDD ativo 2.83", 2.83)):
    for ds in (0.03, 0.06, 0.10):
        eco_wh = kwh_tb_ano / 1000 * 5 * ds * pue * 1000
        print(f"  {nome} dS={ds}: economiza {eco_wh:.2f} Wh/GB")
for thr, w in ((1, 72), (4, 72), (0.25, 72)):
    wh = w / (thr * 3.6) * pue
    print(f"  processar {thr} MB/s em {w} W: {wh:.1f} Wh/GB por passagem (com PUE)")
print()
print("razoes processar/economizar (1 MB/s, 1 passagem de encode):")
proc = 72 / 3.6 * pue
for ds in (0.03, 0.06, 0.10):
    for kwh in (1.89, 2.83):
        eco = kwh / 1000 * 5 * ds * pue * 1000
        print(f"  dS={ds} kwh={kwh}: {proc/eco:.0f}x")
print()
print("== amortização do modelo: D_min = M / dS")
for nome, m_gb, ds in (("ts_zip 169M (8 bit)", 0.169, 0.062), ("L3TC-like 3.2M", 0.0064, 0.048), ("LLM 7B fp16", 14, 0.11), ("Chinchilla 70B fp16", 140, 0.114)):
    print(f"  {nome}: M={m_gb} GB dS={ds} -> D_min = {m_gb/ds:.2f} GB")
print()
print("== latência de reconstrução")
for thr in (0.1, 1, 4):
    for gb in (0.1, 1, 10):
        seg = gb * 1000 / thr
        print(f"  {thr} MB/s, objeto {gb} GB: {seg:,.0f} s = {seg/60:,.1f} min")
print()
print("== 5 TB em throughput neural")
tb = 5e6
for thr in (0.25, 1, 4):
    s = tb/thr
    print(f"  {thr} MB/s: {s/86400:.0f} dias em 1 GPU; GPUs p/ 7 dias: {s/86400/7:.1f}")
print()
print("== classico vs nada (retorno)")
for nome, p, c in (("nuvem S3", 0.023, 0.0088), ("on-prem", 0.002, 0.0040)):
    gan = 0.6 * p * 60
    print(f"  {nome}: ganho 60% = US${gan:.3f}/GB, custo zstd-22 enc = US${c}/GB -> {gan/c:.0f}x")
print()
print("== n* leituras para margem=0")
for nome, ds, p, c in (("nuvem O", .10, .023, .0559), ("nuvem R", .06, .023, .2236), ("onprem O", .10, .002, .0104)):
    g = ds * p * 60
    print(f"  {nome}: ganho={g:.4f} c={c} n*={(g-c)/c:.2f}")
