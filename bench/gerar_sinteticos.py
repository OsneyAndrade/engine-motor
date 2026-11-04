import argparse
import json
import os
import random
import subprocess
import sys
import time

MIB = 1 << 20

NIVEIS = ["INFO"] * 12 + ["DEBUG"] * 4 + ["WARN"] * 2 + ["ERROR"]
SERVICOS = ["api-gateway", "auth", "storage-node", "ingest", "scheduler", "billing", "indexer", "cache"]
METODOS = ["GET"] * 8 + ["PUT"] * 3 + ["POST"] * 2 + ["DELETE"]
CAMINHOS = [
    "/v1/objects/{h}",
    "/v1/buckets/{b}/objects",
    "/v1/users/{u}/profile",
    "/v1/blocks/{h}/replicas",
    "/health",
    "/v2/search?q={b}",
    "/v1/jobs/{u}/status",
]
STATUS = [200] * 70 + [201] * 8 + [204] * 6 + [301] * 2 + [304] * 4 + [400] * 3 + [404] * 4 + [500] * 2 + [503]
HOSTS = ["node-%02d" % i for i in range(24)]
BUCKETS = ["bkt-%04d" % i for i in range(300)]


def gerar_logs(semente, tamanho_alvo):
    rng = random.Random(semente)
    pool_ips = ["10.%d.%d.%d" % (rng.randrange(0, 40), rng.randrange(0, 256), rng.randrange(1, 255)) for _ in range(3000)]
    pesos_ips = [1.0 / (i + 1) ** 0.8 for i in range(len(pool_ips))]
    usuarios = ["u%05d" % rng.randrange(0, 60000) for _ in range(5000)]
    pids = {h: rng.randrange(500, 40000) for h in HOSTS}
    instante_ms = 1793577600000 + semente * 86400000
    saida = []
    total = 0
    while total < tamanho_alvo:
        lote_ips = rng.choices(pool_ips, weights=pesos_ips, k=512)
        for ip in lote_ips:
            instante_ms += rng.randrange(0, 40)
            segundos, ms = divmod(instante_ms, 1000)
            t = rng.randrange(0, 1 << 32)
            h = HOSTS[(t >> 3) % len(HOSTS)]
            servico = SERVICOS[(t >> 9) % len(SERVICOS)]
            nivel = NIVEIS[(t >> 13) % len(NIVEIS)]
            caminho = CAMINHOS[(t >> 17) % len(CAMINHOS)].format(
                h="%016x" % rng.getrandbits(64), b=BUCKETS[(t >> 21) % len(BUCKETS)], u=usuarios[(t >> 5) % len(usuarios)]
            )
            estado = STATUS[(t >> 25) % len(STATUS)]
            linha = "%04d-%02d-%02dT%02d:%02d:%02d.%03dZ %s %s[%d]: %s req=%08x ip=%s user=%s %s %s %d %dms bytes=%d\n" % (
                *_data_hora(segundos),
                ms,
                h,
                servico,
                pids[h],
                nivel,
                rng.getrandbits(32),
                ip,
                usuarios[(t >> 7) % len(usuarios)],
                METODOS[(t >> 27) % len(METODOS)],
                caminho,
                estado,
                int(rng.lognormvariate(2.5, 1.0)),
                int(rng.lognormvariate(7.0, 1.6)),
            )
            saida.append(linha)
            total += len(linha)
    return "".join(saida).encode("ascii")[:tamanho_alvo]


def _data_hora(segundos):
    g = time.gmtime(segundos)
    return g.tm_year, g.tm_mon, g.tm_mday, g.tm_hour, g.tm_min, g.tm_sec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--saida", required=True)
    ap.add_argument("--texto-fonte", required=True)
    ap.add_argument("--semente", type=int, default=20261103)
    ap.add_argument("--logs-mib", type=int, default=64)
    ap.add_argument("--aleatorio-mib", type=int, default=32)
    a = ap.parse_args()
    os.makedirs(a.saida, exist_ok=True)
    if os.listdir(a.saida):
        sys.exit("diretorio de saida precisa estar vazio: " + a.saida)

    rng = random.Random(a.semente)
    with open(os.path.join(a.saida, "aleatorio.bin"), "wb") as f:
        f.write(rng.randbytes(a.aleatorio_mib * MIB))

    with open(os.path.join(a.saida, "logs.log"), "wb") as f:
        f.write(gerar_logs(1, a.logs_mib * MIB))
    with open(os.path.join(a.saida, "logs_treino.log"), "wb") as f:
        f.write(gerar_logs(2, 8 * MIB))

    with open(a.texto_fonte, "rb") as src, open(os.path.join(a.saida, "ja_comprimido.xz"), "wb") as dst:
        subprocess.run(["xz", "-9", "-T1", "-c"], stdin=src, stdout=dst, check=True)

    info = {
        "semente": a.semente,
        "aleatorio": "random.Random(semente).randbytes(%d MiB)" % a.aleatorio_mib,
        "logs": "gerar_logs(semente=1, %d MiB)" % a.logs_mib,
        "logs_treino": "gerar_logs(semente=2, 8 MiB), disjunto de logs.log",
        "ja_comprimido": "xz -9 de " + os.path.basename(a.texto_fonte),
    }
    print(json.dumps(info))


if __name__ == "__main__":
    main()
