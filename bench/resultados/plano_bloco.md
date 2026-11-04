# Plano por bloco: um classificador barato escolhe transform + codec + nivel?

Gerado por `python3 -I bench/plano_bloco.py relatorio` em 2026-11-04 10:44. Numeros brutos em `plano_bloco.json` (por granularidade: resumo por plano e por classe, acuracias, matrizes de confusao, modelos finais com pesos/nos portaveis, tempos) e rotulos por bloco em `plano_bloco_rotulos.npz`. Metodologia, definicoes e limitacoes no `bench/README.md` (secao Rodada 3).

## 1. Veredito (criterio de parada)

- **64K**: NAO VALE com os numeros medidos: reprova (iii) custo de features (melhor classificador: logit).
- **256K**: NAO VALE com os numeros medidos: reprova (ii) ganho sobre o probe, (iii) custo de features (melhor classificador: logit).
- **1M**: NAO VALE com os numeros medidos: reprova (ii) ganho sobre o probe, (iii) custo de features (melhor classificador: cart).

Detalhe por condicao (regret e ganho em leave-one-file-out agrupado; IC95 = bootstrap sobre os arquivos, 2000 reamostragens):

| gran. | modelo | (i) regret de bytes <= 1% (LOFO global) | (ii) ganho >= 1% sobre probe+zstd-3 | (iii) features+inferencia <= 2% do zstd-3 | as 3 pelo medido |
|---|---|---|---|---|---|
| 64K | CART | 0,62% [IC95 0,23; 1,14] OK | 0,96% [IC95 -0,16; 3,28] FALHA | 90,5% FALHA; nativo estimado 30,3% | NAO |
| 64K | logistica | 0,39% [IC95 0,15; 0,70] OK | 1,18% [IC95 0,07; 3,42] OK | 123,6% FALHA; nativo estimado 37,0% | NAO |
| 64K | regra manual | 3,03% [IC95 0,31; 7,54] FALHA | -1,42% [IC95 -6,13; 2,58] FALHA | 56,8% FALHA; nativo estimado 11,8% | NAO |
| 256K | CART | 1,66% [IC95 0,37; 3,59] FALHA | 0,06% [IC95 -0,27; 0,44] FALHA | 23,9% FALHA; nativo estimado 7,6% | NAO |
| 256K | logistica | 0,76% [IC95 0,14; 1,71] OK | 0,95% [IC95 0,12; 2,59] FALHA | 31,2% FALHA; nativo estimado 8,8% | NAO |
| 256K | regra manual | 4,11% [IC95 0,48; 9,88] FALHA | -2,35% [IC95 -8,13; 1,93] FALHA | 13,9% FALHA; nativo estimado 2,8% | NAO |
| 1M | CART | 0,47% [IC95 0,11; 1,05] OK | 0,92% [IC95 -0,02; 2,80] FALHA | 3,2% FALHA; nativo estimado 1,4% | NAO |
| 1M | logistica | 0,80% [IC95 0,07; 2,44] OK | 0,60% [IC95 -0,00; 1,34] FALHA | 8,4% FALHA; nativo estimado 2,4% | NAO |
| 1M | regra manual | 3,35% [IC95 0,27; 8,97] FALHA | -1,91% [IC95 -7,15; 2,12] FALHA | 3,7% FALHA; nativo estimado 0,7% | NAO |

Probe+zstd-3 e oraculo de tamanho do caminho sincrono:

| gran. | melhor probe (por bytes) | regret do probe | probe a <= 1% do oraculo? | melhor classificador | veredito |
|---|---|---|---|---|---|
| 64K | probe+zstd-3 (espalhado) | 1,59% | nao | logit | NAO VALE com os numeros medidos: reprova (iii) custo de features |
| 256K | probe+zstd-3 (espalhado) | 1,72% | nao | logit | NAO VALE com os numeros medidos: reprova (ii) ganho sobre o probe, (iii) custo de features |
| 1M | probe+zstd-3 (espalhado) | 1,41% | nao | cart | NAO VALE com os numeros medidos: reprova (ii) ganho sobre o probe, (iii) custo de features |


## 2. Dados e bit-exatidao

Corpus: 34 arquivos/grupos, 574725218 bytes. Blocos independentes: {"64K": 8782, "256K": 2207, "1M": 562}. Comparacoes (bloco, candidato) com decodificacao e sha256 do bloco: {"64K": 106188, "256K": 26678, "1M": 6786}. **Falhas de reconstrucao: 0** (blocos nao exatos por granularidade: {"64K": 0, "256K": 0, "1M": 0}); a reversibilidade `invert(apply(x)) == x` dos transforms foi verificada em todo bloco e tambem contra uma implementacao de referencia ingenua da semantica de `src/transform.rs` em 846 casos de borda (`autoteste`).

Classe `codigo_kernel`: primeiros 268435456 bytes do fluxo tar de `https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.12.115.tar.xz` (streaming com `lzma`, 34603008 bytes de xz baixados, 242577992 bytes de conteudo de 32427 arquivos regulares agrupados em 16 pseudo-arquivos por diretorio; o sha256 do tar.xz inteiro NAO foi conferido porque so o inicio foi baixado).

| arquivo | classe | bytes | blocos 64K | blocos 256K | blocos 1M |
|---|---|---|---|---|---|
| dickens | texto | 10192446 | 156 | 39 | 10 |
| mozilla | binario | 51220480 | 782 | 196 | 49 |
| mr | cientifico | 9970564 | 153 | 39 | 10 |
| nci | cientifico | 33553445 | 512 | 128 | 32 |
| ooffice | binario | 6152192 | 94 | 24 | 6 |
| osdb | banco_de_dados | 10085684 | 154 | 39 | 10 |
| reymont | texto | 6627202 | 102 | 26 | 7 |
| samba | codigo | 21606400 | 330 | 83 | 21 |
| sao | cientifico | 7251944 | 111 | 28 | 7 |
| webster | texto | 41458703 | 633 | 159 | 40 |
| x-ray | cientifico | 8474240 | 130 | 33 | 9 |
| xml | xml | 5345280 | 82 | 21 | 6 |
| aleatorio.bin | sint_incompressivel | 33554432 | 512 | 128 | 32 |
| logs.log | sint_logs | 67108864 | 1024 | 256 | 64 |
| ja_comprimido.xz | sint_ja_comprimido | 8385868 | 128 | 32 | 8 |
| bible.txt | texto | 4047392 | 62 | 16 | 4 |
| E.coli | dna | 4638690 | 71 | 18 | 5 |
| world192.txt | texto | 2473400 | 38 | 10 | 3 |
| kernel/kernel_Documentation__resto | codigo_kernel | 35151761 | 537 | 135 | 34 |
| kernel/kernel_Documentation_devicetree | codigo_kernel | 14403731 | 220 | 55 | 14 |
| kernel/kernel_LICENSES | codigo_kernel | 228044 | 4 | 1 | 1 |
| kernel/kernel_arch__resto | codigo_kernel | 25206732 | 385 | 97 | 25 |
| kernel/kernel_arch_arm | codigo_kernel | 27183008 | 415 | 104 | 26 |
| kernel/kernel_arch_arm64 | codigo_kernel | 23550642 | 360 | 90 | 23 |
| kernel/kernel_arch_mips | codigo_kernel | 8901329 | 136 | 34 | 9 |
| kernel/kernel_arch_powerpc | codigo_kernel | 14395994 | 220 | 55 | 14 |
| kernel/kernel_arch_x86 | codigo_kernel | 13692593 | 209 | 53 | 14 |
| kernel/kernel_block | codigo_kernel | 1966835 | 31 | 8 | 2 |
| kernel/kernel_certs | codigo_kernel | 40307 | 1 | 1 | 1 |
| kernel/kernel_crypto | codigo_kernel | 3566168 | 55 | 14 | 4 |
| kernel/kernel_drivers__resto | codigo_kernel | 39696395 | 606 | 152 | 38 |
| kernel/kernel_drivers_accel | codigo_kernel | 13645295 | 209 | 53 | 14 |
| kernel/kernel_drivers_clk | codigo_kernel | 19917505 | 304 | 76 | 19 |
| kernel/kernel_raiz | codigo_kernel | 1031653 | 16 | 4 | 1 |


## 3. Oraculo

### 3.1 Distribuicao do oraculo de tamanho (caminho sincrono)

Granularidade 64K (oraculo de tamanho do caminho sincrono; % dos blocos da classe cujo menor tamanho e do plano):

| classe | blocos | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|---|
| texto | 991 | 0 | 0% | 100% | 0 | 0 | 0 | 0 | 0 | 0 |
| codigo | 330 | 1% | 7% | 80% | 5% | 0 | 0 | 0 | 7% | 0 |
| codigo_kernel | 3708 | 0 | 3% | 96% | 0 | 0 | 0 | 0 | 1% | 0% |
| binario | 876 | 1% | 4% | 87% | 0% | 1% | 0 | 1% | 3% | 4% |
| cientifico | 906 | 0 | 39% | 30% | 0 | 10% | 0 | 0 | 21% | 0% |
| banco_de_dados | 154 | 0 | 0 | 100% | 0 | 0 | 0 | 0 | 0 | 0 |
| xml | 82 | 0 | 13% | 85% | 1% | 0 | 0 | 0 | 0 | 0 |
| dna | 71 | 0 | 100% | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_logs | 1024 | 0 | 100% | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_incompressivel | 512 | 100% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_ja_comprimido | 128 | 100% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

Granularidade 256K (oraculo de tamanho do caminho sincrono; % dos blocos da classe cujo menor tamanho e do plano):

| classe | blocos | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|---|
| texto | 250 | 0 | 0% | 100% | 0 | 0 | 0 | 0 | 0 | 0 |
| codigo | 83 | 0 | 10% | 81% | 5% | 0 | 0 | 0 | 5% | 0 |
| codigo_kernel | 932 | 0 | 2% | 97% | 0 | 0 | 0 | 0 | 1% | 0% |
| binario | 220 | 0 | 3% | 91% | 0 | 1% | 0 | 1% | 2% | 2% |
| cientifico | 228 | 0 | 53% | 16% | 0 | 11% | 0 | 0 | 20% | 0 |
| banco_de_dados | 39 | 0 | 0 | 100% | 0 | 0 | 0 | 0 | 0 | 0 |
| xml | 21 | 0 | 19% | 81% | 0 | 0 | 0 | 0 | 0 | 0 |
| dna | 18 | 0 | 100% | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_logs | 256 | 0 | 100% | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_incompressivel | 128 | 100% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_ja_comprimido | 32 | 100% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

Granularidade 1M (oraculo de tamanho do caminho sincrono; % dos blocos da classe cujo menor tamanho e do plano):

| classe | blocos | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|---|
| texto | 64 | 0 | 0 | 100% | 0 | 0 | 0 | 0 | 0 | 0 |
| codigo | 21 | 0 | 5% | 90% | 0 | 0 | 0 | 0 | 5% | 0 |
| codigo_kernel | 239 | 0 | 2% | 97% | 0 | 0 | 0 | 0 | 0% | 0 |
| binario | 55 | 0 | 0 | 100% | 0 | 0 | 0 | 0 | 0 | 0 |
| cientifico | 58 | 0 | 48% | 19% | 0 | 14% | 0 | 0 | 19% | 0 |
| banco_de_dados | 10 | 0 | 0 | 100% | 0 | 0 | 0 | 0 | 0 | 0 |
| xml | 6 | 0 | 0 | 100% | 0 | 0 | 0 | 0 | 0 | 0 |
| dna | 5 | 0 | 20% | 80% | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_logs | 64 | 0 | 100% | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_incompressivel | 32 | 100% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_ja_comprimido | 8 | 100% | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |


### 3.2 Caminho assincrono

- 64K: oraculo assincrono = 163113992 B; oraculo sincrono = 189649034 B; zstd-3 fixo = 192534153 B; oraculo sincrono ou assincrono = 163113524 B. Ganho do assincrono sobre o sincrono: 13,99%.
- 256K: oraculo assincrono = 149925193 B; oraculo sincrono = 181117759 B; zstd-3 fixo = 183982280 B; oraculo sincrono ou assincrono = 149925193 B. Ganho do assincrono sobre o sincrono: 17,22%.
- 1M: oraculo assincrono = 142860770 B; oraculo sincrono = 175948519 B; zstd-3 fixo = 177917742 B; oraculo sincrono ou assincrono = 142860770 B. Ganho do assincrono sobre o sincrono: 18,81%.

Bytes por candidato assincrono (256K), reducao % sobre zstd-3 da mesma classe e o plano que mais vence:

| classe | zstd-3 (MB) | zstd-19 | xz-9e | bzip2-9 | vence mais |
|---|---|---|---|---|---|
| texto | 21,23 | 17,0% | 18,1% | 27,6% | bzip2-9 (100%) |
| codigo | 5,45 | 14,1% | 16,9% | 12,0% | xz-9e (70%) |
| codigo_kernel | 51,48 | 17,4% | 20,4% | 18,1% | xz-9e (70%) |
| binario | 22,18 | 12,1% | 20,1% | 4,1% | xz-9e (90%) |
| cientifico | 18,37 | 12,2% | 23,1% | 25,1% | bzip2-9 (71%) |
| banco_de_dados | 3,71 | 5,1% | 12,2% | 14,4% | bzip2-9 (97%) |
| xml | 0,73 | 22,9% | 25,8% | 32,6% | bzip2-9 (100%) |
| dna | 1,47 | 20,9% | 15,0% | 14,6% | zstd-19 (100%) |
| sint_logs | 17,41 | 23,1% | 24,1% | 32,5% | bzip2-9 (100%) |
| sint_incompressivel | 33,55 | 0,0% | 0,0% | 0,0% | stored (100%) |
| sint_ja_comprimido | 8,39 | 0,0% | 0,0% | 0,0% | stored (100%) |


## 4. Triagem de amostragem, tamanho da amostra e pesos

Variante escolhida: **espalhado, amostra de 4096 B, pesos por custo = sim** (menor regret medio de bytes (CART, LOFO, rotulo tamanho) nas 3 granularidades, com +0,05 ponto de penalidade para amostra de 16 KiB).

| gran. | amostragem | amostra (B) | pesos por custo | regret de bytes (CART LOFO) | acerto estrito |
|---|---|---|---|---|---|
| 1M | inicio | 4096 | nao | 2,61% | 73,5% |
| 1M | inicio | 4096 | sim | 1,92% | 69,6% |
| 1M | espalhado | 4096 | nao | 0,98% | 76,9% |
| 1M | espalhado | 4096 | sim | 0,47% | 72,6% |
| 1M | inicio | 16384 | nao | 1,40% | 77,2% |
| 1M | inicio | 16384 | sim | 1,47% | 71,7% |
| 1M | espalhado | 16384 | nao | 1,70% | 76,3% |
| 1M | espalhado | 16384 | sim | 0,83% | 75,8% |
| 256K | inicio | 4096 | nao | 2,99% | 72,2% |
| 256K | inicio | 4096 | sim | 1,47% | 74,4% |
| 256K | espalhado | 4096 | nao | 2,10% | 80,0% |
| 256K | espalhado | 4096 | sim | 1,66% | 78,6% |
| 256K | inicio | 16384 | nao | 2,08% | 72,5% |
| 256K | inicio | 16384 | sim | 1,10% | 74,0% |
| 256K | espalhado | 16384 | nao | 1,70% | 80,0% |
| 256K | espalhado | 16384 | sim | 2,17% | 81,0% |
| 64K | inicio | 4096 | nao | 1,60% | 76,6% |
| 64K | inicio | 4096 | sim | 2,11% | 71,8% |
| 64K | espalhado | 4096 | nao | 2,17% | 75,7% |
| 64K | espalhado | 4096 | sim | 0,62% | 80,1% |
| 64K | inicio | 16384 | nao | 2,34% | 73,3% |
| 64K | inicio | 16384 | sim | 0,99% | 75,9% |
| 64K | espalhado | 16384 | nao | 1,18% | 76,9% |
| 64K | espalhado | 16384 | sim | 0,95% | 76,4% |


## 5. Modelos (tamanho)

| gran. | nos do CART | folhas | features usadas pelo CART | pesos da logistica | forma (21 features + vies) x 9 planos |
|---|---|---|---|---|---|
| 64K | 73 | 37 | h0, h1, nd, fz, fp, fmax, run, hd1, hd2, hd8, hp4, ac2, cr2, cr4, cr8, pr | 198 | 22x9 |
| 256K | 51 | 26 | h0, h1, nd, fp, fmax, hd1, hd2, hd4, hd8, hp4, ac2, ac4, cr2, cr4, cr8, pr | 198 | 22x9 |
| 1M | 23 | 12 | h0, nd, run, hd4, hp8, cr2, cr4, pr | 198 | 22x9 |


## 6. Acuracia top-1

Granularidade 64K, acuracia top-1 estrita / tolerante (%; tolerante = plano a <= 0,5% do valor do oraculo). Para os modelos, cada coluna usa o modelo treinado com aquele rotulo:

| plano | oraculo de tamanho | oraculo economico nuvem+S3 morno | oraculo economico on-prem morno |
|---|---|---|---|
| zstd-3 fixo | 69,0 / 77,8 | 69,0 / 85,6 | 68,9 / 85,6 |
| zstd-1 fixo | 18,7 / 27,3 | 18,7 / 24,9 | 18,8 / 24,9 |
| probe+zstd-3 (inicio) | 76,1 / 77,5 | 76,1 / 92,7 | 76,0 / 92,7 |
| probe+zstd-3 (espalhado) | 76,2 / 77,5 | 76,2 / 92,7 | 76,0 / 92,7 |
| regra manual | 69,4 / 70,7 | 69,3 / 85,9 | 69,2 / 85,9 |
| CART LOFO | 80,1 / 81,8 | 76,4 / 91,1 | 76,6 / 91,2 |
| CART LOCO | 79,5 / 81,1 | 74,4 / 89,9 | 74,5 / 89,8 |
| logistica LOFO | 72,5 / 81,5 | 71,8 / 86,8 | 71,9 / 86,7 |
| logistica LOCO | 66,0 / 74,9 | 64,7 / 80,9 | 64,8 / 80,9 |

Granularidade 64K, acuracia estrita (%) vs oraculo de tamanho, por classe:

| classe | zstd-3 fixo | probe (espalhado) | CART LOFO | CART LOCO | logistica LOFO | logistica LOCO |
|---|---|---|---|---|---|---|
| texto | 100 | 100 | 100 | 99 | 100 | 100 |
| codigo | 80 | 80 | 79 | 79 | 79 | 79 |
| codigo_kernel | 96 | 96 | 95 | 95 | 95 | 79 |
| binario | 87 | 85 | 79 | 81 | 87 | 87 |
| cientifico | 30 | 30 | 71 | 64 | 62 | 65 |
| banco_de_dados | 100 | 100 | 100 | 100 | 100 | 100 |
| xml | 85 | 85 | 83 | 83 | 76 | 76 |
| dna | 0 | 0 | 100 | 100 | 100 | 100 |
| sint_logs | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_incompressivel | 0 | 100 | 100 | 100 | 0 | 0 |
| sint_ja_comprimido | 0 | 100 | 88 | 88 | 0 | 0 |

Granularidade 256K, acuracia top-1 estrita / tolerante (%; tolerante = plano a <= 0,5% do valor do oraculo). Para os modelos, cada coluna usa o modelo treinado com aquele rotulo:

| plano | oraculo de tamanho | oraculo economico nuvem+S3 morno | oraculo economico on-prem morno |
|---|---|---|---|
| zstd-3 fixo | 68,5 / 76,3 | 68,5 / 74,8 | 68,5 / 74,7 |
| zstd-1 fixo | 19,8 / 27,4 | 19,8 / 23,3 | 19,8 / 23,3 |
| probe+zstd-3 (inicio) | 75,1 / 75,6 | 75,1 / 81,5 | 75,1 / 81,4 |
| probe+zstd-3 (espalhado) | 75,4 / 75,9 | 75,4 / 81,8 | 75,4 / 81,7 |
| regra manual | 67,9 / 68,5 | 67,9 / 74,2 | 67,9 / 74,1 |
| CART LOFO | 78,6 / 80,7 | 72,8 / 73,9 | 73,1 / 74,3 |
| CART LOCO | 72,0 / 73,9 | 71,6 / 73,0 | 71,6 / 73,0 |
| logistica LOFO | 74,7 / 82,5 | 74,2 / 76,3 | 74,2 / 76,3 |
| logistica LOCO | 68,6 / 76,2 | 67,4 / 70,3 | 67,4 / 70,3 |

Granularidade 256K, acuracia estrita (%) vs oraculo de tamanho, por classe:

| classe | zstd-3 fixo | probe (espalhado) | CART LOFO | CART LOCO | logistica LOFO | logistica LOCO |
|---|---|---|---|---|---|---|
| texto | 100 | 100 | 96 | 76 | 100 | 100 |
| codigo | 81 | 81 | 76 | 76 | 81 | 81 |
| codigo_kernel | 97 | 97 | 97 | 97 | 96 | 79 |
| binario | 91 | 89 | 79 | 24 | 91 | 91 |
| cientifico | 16 | 16 | 67 | 77 | 70 | 81 |
| banco_de_dados | 100 | 100 | 100 | 100 | 100 | 100 |
| xml | 81 | 81 | 90 | 90 | 95 | 95 |
| dna | 0 | 0 | 100 | 100 | 100 | 100 |
| sint_logs | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_incompressivel | 0 | 100 | 73 | 73 | 0 | 0 |
| sint_ja_comprimido | 0 | 100 | 94 | 94 | 0 | 0 |

Granularidade 1M, acuracia top-1 estrita / tolerante (%; tolerante = plano a <= 0,5% do valor do oraculo). Para os modelos, cada coluna usa o modelo treinado com aquele rotulo:

| plano | oraculo de tamanho | oraculo economico nuvem+S3 morno | oraculo economico on-prem morno |
|---|---|---|---|
| zstd-3 fixo | 71,5 / 80,4 | 71,5 / 88,6 | 71,5 / 88,6 |
| zstd-1 fixo | 17,6 / 25,3 | 17,6 / 21,0 | 17,6 / 21,0 |
| probe+zstd-3 (inicio) | 78,1 / 79,9 | 78,1 / 95,2 | 78,1 / 95,2 |
| probe+zstd-3 (espalhado) | 78,1 / 79,9 | 78,1 / 95,2 | 78,1 / 95,2 |
| regra manual | 69,4 / 71,5 | 69,4 / 86,8 | 69,4 / 86,8 |
| CART LOFO | 72,6 / 81,0 | 76,2 / 90,2 | 76,0 / 90,0 |
| CART LOCO | 71,7 / 79,5 | 71,7 / 85,4 | 71,7 / 85,4 |
| logistica LOFO | 74,2 / 82,2 | 74,6 / 88,4 | 74,6 / 88,4 |
| logistica LOCO | 67,3 / 74,7 | 67,4 / 81,3 | 67,4 / 81,3 |

Granularidade 1M, acuracia estrita (%) vs oraculo de tamanho, por classe:

| classe | zstd-3 fixo | probe (espalhado) | CART LOFO | CART LOCO | logistica LOFO | logistica LOCO |
|---|---|---|---|---|---|---|
| texto | 100 | 100 | 100 | 92 | 100 | 100 |
| codigo | 90 | 90 | 90 | 90 | 90 | 90 |
| codigo_kernel | 97 | 97 | 97 | 97 | 96 | 79 |
| binario | 100 | 95 | 96 | 96 | 100 | 100 |
| cientifico | 19 | 19 | 33 | 31 | 57 | 60 |
| banco_de_dados | 100 | 100 | 100 | 100 | 100 | 100 |
| xml | 100 | 100 | 100 | 100 | 100 | 100 |
| dna | 80 | 80 | 20 | 20 | 20 | 20 |
| sint_logs | 0 | 0 | 0 | 0 | 0 | 0 |
| sint_incompressivel | 0 | 100 | 0 | 0 | 0 | 0 |
| sint_ja_comprimido | 0 | 100 | 62 | 62 | 0 | 0 |



## 7. Regret global por plano, granularidade e camada

Granularidade 64K (8782 blocos, validacao leave-one-file-out; regret em US$ por GB logico, positivo = perdeu valor liquido vs o oraculo economico do mesmo cenario e camada):

| plano | regret bytes | US$/GB nuvem|quente | US$/GB nuvem|morno | US$/GB nuvem|frio | US$/GB on-prem|quente | US$/GB on-prem|morno | US$/GB on-prem|frio |
|---|---|---|---|---|---|---|---|
| zstd-3 fixo | 1,52% | 0,00689 | 0,00693 | 0,00693 | 0,00059 | 0,00060 | 0,00060 |
| zstd-1 fixo | 4,78% | 0,02170 | 0,02176 | 0,02176 | 0,00187 | 0,00189 | 0,00189 |
| probe+zstd-3 (inicio) | 1,68% | 0,00760 | 0,00764 | 0,00764 | 0,00065 | 0,00066 | 0,00066 |
| probe+zstd-3 (espalhado) | 1,59% | 0,00722 | 0,00726 | 0,00726 | 0,00062 | 0,00063 | 0,00063 |
| regra manual | 3,03% | 0,01382 | 0,01381 | 0,01381 | 0,00121 | 0,00120 | 0,00120 |
| CART prof. 6 (treino: tamanho) | 0,62% | 0,00281 | 0,00282 | 0,00282 | 0,00024 | 0,00025 | 0,00025 |
| CART prof. 6 (treino: economico, diagonal) | - | 0,00817 | 0,00818 | 0,00819 | 0,00070 | 0,00071 | 0,00071 |
| regressao logistica (treino: tamanho) | 0,39% | 0,00177 | 0,00179 | 0,00179 | 0,00015 | 0,00016 | 0,00016 |
| regressao logistica (treino: economico, diagonal) | - | 0,00192 | 0,00194 | 0,00193 | 0,00016 | 0,00017 | 0,00017 |
| medir todos (custo de medir incluido) | 0,00% | 0,00019 | 0,00019 | 0,00019 | 0,00006 | 0,00006 | 0,00006 |
| oraculo (tamanho) | 0,00% | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |

Granularidade 256K (2207 blocos, validacao leave-one-file-out; regret em US$ por GB logico, positivo = perdeu valor liquido vs o oraculo economico do mesmo cenario e camada):

| plano | regret bytes | US$/GB nuvem|quente | US$/GB nuvem|morno | US$/GB nuvem|frio | US$/GB on-prem|quente | US$/GB on-prem|morno | US$/GB on-prem|frio |
|---|---|---|---|---|---|---|---|
| zstd-3 fixo | 1,58% | 0,00684 | 0,00688 | 0,00688 | 0,00059 | 0,00060 | 0,00060 |
| zstd-1 fixo | 4,99% | 0,02163 | 0,02169 | 0,02169 | 0,00187 | 0,00188 | 0,00188 |
| probe+zstd-3 (inicio) | 2,21% | 0,00955 | 0,00960 | 0,00960 | 0,00082 | 0,00083 | 0,00084 |
| probe+zstd-3 (espalhado) | 1,72% | 0,00745 | 0,00749 | 0,00749 | 0,00064 | 0,00065 | 0,00065 |
| regra manual | 4,11% | 0,01791 | 0,01790 | 0,01789 | 0,00156 | 0,00156 | 0,00156 |
| CART prof. 6 (treino: tamanho) | 1,66% | 0,00721 | 0,00724 | 0,00724 | 0,00062 | 0,00063 | 0,00063 |
| CART prof. 6 (treino: economico, diagonal) | - | 0,00790 | 0,00788 | 0,00787 | 0,00069 | 0,00068 | 0,00069 |
| regressao logistica (treino: tamanho) | 0,76% | 0,00328 | 0,00330 | 0,00330 | 0,00028 | 0,00029 | 0,00029 |
| regressao logistica (treino: economico, diagonal) | - | 0,00335 | 0,00337 | 0,00337 | 0,00028 | 0,00029 | 0,00029 |
| medir todos (custo de medir incluido) | 0,00% | 0,00019 | 0,00019 | 0,00019 | 0,00006 | 0,00006 | 0,00006 |
| oraculo (tamanho) | 0,00% | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |

Granularidade 1M (562 blocos, validacao leave-one-file-out; regret em US$ por GB logico, positivo = perdeu valor liquido vs o oraculo economico do mesmo cenario e camada):

| plano | regret bytes | US$/GB nuvem|quente | US$/GB nuvem|morno | US$/GB nuvem|frio | US$/GB on-prem|quente | US$/GB on-prem|morno | US$/GB on-prem|frio |
|---|---|---|---|---|---|---|---|
| zstd-3 fixo | 1,12% | 0,00470 | 0,00473 | 0,00473 | 0,00040 | 0,00041 | 0,00041 |
| zstd-1 fixo | 7,63% | 0,03216 | 0,03221 | 0,03222 | 0,00278 | 0,00280 | 0,00280 |
| probe+zstd-3 (inicio) | 1,91% | 0,00802 | 0,00806 | 0,00806 | 0,00069 | 0,00070 | 0,00070 |
| probe+zstd-3 (espalhado) | 1,41% | 0,00590 | 0,00594 | 0,00594 | 0,00051 | 0,00052 | 0,00052 |
| regra manual | 3,35% | 0,01416 | 0,01414 | 0,01414 | 0,00124 | 0,00123 | 0,00123 |
| CART prof. 6 (treino: tamanho) | 0,47% | 0,00197 | 0,00199 | 0,00199 | 0,00017 | 0,00017 | 0,00017 |
| CART prof. 6 (treino: economico, diagonal) | - | 0,00216 | 0,00188 | 0,00218 | 0,00015 | 0,00019 | 0,00016 |
| regressao logistica (treino: tamanho) | 0,80% | 0,00336 | 0,00338 | 0,00339 | 0,00029 | 0,00029 | 0,00029 |
| regressao logistica (treino: economico, diagonal) | - | 0,00417 | 0,00420 | 0,00420 | 0,00036 | 0,00036 | 0,00037 |
| medir todos (custo de medir incluido) | 0,00% | 0,00019 | 0,00019 | 0,00019 | 0,00006 | 0,00006 | 0,00006 |
| oraculo (tamanho) | 0,00% | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |



## 8. Regret por classe

Granularidade 64K, regret de bytes (%) vs oraculo de tamanho do caminho sincrono, por classe:

| classe | zstd-3 fixo | zstd-1 fixo | probe+zstd-3 (inicio) | probe+zstd-3 (espalhado) | regra manual | CART | logistica |
|---|---|---|---|---|---|---|---|
| texto | 0,01 | 5,72 | 0,01 | 0,01 | 0,01 | 0,00 | 0,01 |
| codigo | 1,65 | 4,33 | 3,66 | 2,63 | 1,50 | 2,40 | 1,41 |
| codigo_kernel | 0,14 | 5,67 | 0,14 | 0,14 | 0,13 | 0,40 | 0,15 |
| binario | 0,88 | 4,29 | 1,66 | 1,24 | 18,83 | 1,73 | 0,77 |
| cientifico | 13,37 | 18,49 | 13,37 | 13,37 | 5,89 | 1,35 | 1,25 |
| banco_de_dados | 0,00 | 4,27 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 |
| xml | 0,05 | 6,00 | 0,05 | 0,05 | 0,05 | 0,10 | 0,78 |
| dna | 3,43 | 0,00 | 3,43 | 3,43 | 3,43 | 0,00 | 0,00 |
| sint_logs | 1,08 | 0,00 | 1,08 | 1,08 | 1,08 | 1,08 | 1,08 |
| sint_incompressivel | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 |
| sint_ja_comprimido | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 |
| GLOBAL | 1,52 | 4,78 | 1,68 | 1,59 | 3,03 | 0,62 | 0,39 |
| MEDIA DAS CLASSES | 1,87 | 4,43 | 2,13 | 2,00 | 2,81 | 0,64 | 0,49 |

Granularidade 64K, regret em US$/GB (nuvem+S3 Standard, camada morna; modelos treinados com o rotulo economico dessa celula):

| classe | zstd-3 fixo | zstd-1 fixo | probe+zstd-3 (inicio) | probe+zstd-3 (espalhado) | regra manual | CART | logistica |
|---|---|---|---|---|---|---|---|
| texto | 0,00003 | 0,02702 | 0,00003 | 0,00003 | 0,00003 | 0,00002 | 0,00003 |
| codigo | 0,00604 | 0,01586 | 0,01343 | 0,00966 | 0,00550 | 0,00605 | 0,00306 |
| codigo_kernel | 0,00046 | 0,01812 | 0,00046 | 0,00046 | 0,00041 | 0,00063 | 0,00053 |
| binario | 0,00490 | 0,02389 | 0,00927 | 0,00688 | 0,10488 | 0,02587 | 0,00480 |
| cientifico | 0,05291 | 0,07313 | 0,05291 | 0,05291 | 0,02331 | 0,04403 | 0,00646 |
| banco_de_dados | 0,00000 | 0,02445 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |
| xml | 0,00010 | 0,01247 | 0,00010 | 0,00010 | 0,00010 | 0,00129 | 0,00184 |
| dna | 0,01458 | 0,00000 | 0,01458 | 0,01458 | 0,01458 | 0,01333 | 0,00000 |
| sint_logs | 0,00377 | 0,00000 | 0,00377 | 0,00377 | 0,00377 | 0,00377 | 0,00377 |
| sint_incompressivel | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |
| sint_ja_comprimido | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |

Granularidade 256K, regret de bytes (%) vs oraculo de tamanho do caminho sincrono, por classe:

| classe | zstd-3 fixo | zstd-1 fixo | probe+zstd-3 (inicio) | probe+zstd-3 (espalhado) | regra manual | CART | logistica |
|---|---|---|---|---|---|---|---|
| texto | 0,01 | 4,34 | 0,01 | 0,01 | 0,01 | 0,15 | 0,01 |
| codigo | 1,31 | 4,35 | 8,08 | 2,69 | 1,97 | 2,75 | 1,75 |
| codigo_kernel | 0,14 | 6,41 | 0,14 | 0,14 | 0,11 | 0,06 | 0,11 |
| binario | 0,40 | 6,23 | 3,88 | 1,23 | 23,57 | 2,31 | 0,45 |
| cientifico | 10,96 | 18,86 | 10,96 | 10,96 | 7,64 | 9,20 | 2,18 |
| banco_de_dados | 0,00 | 1,18 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 |
| xml | 0,62 | 6,19 | 0,62 | 0,62 | 0,62 | 0,79 | 0,33 |
| dna | 3,55 | 0,00 | 3,55 | 3,55 | 3,55 | 0,00 | 0,00 |
| sint_logs | 4,58 | 0,00 | 4,58 | 4,58 | 4,58 | 4,58 | 4,58 |
| sint_incompressivel | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 |
| sint_ja_comprimido | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 |
| GLOBAL | 1,58 | 4,99 | 2,21 | 1,72 | 4,11 | 1,66 | 0,76 |
| MEDIA DAS CLASSES | 1,96 | 4,32 | 2,89 | 2,16 | 3,82 | 1,80 | 0,85 |

Granularidade 256K, regret em US$/GB (nuvem+S3 Standard, camada morna; modelos treinados com o rotulo economico dessa celula):

| classe | zstd-3 fixo | zstd-1 fixo | probe+zstd-3 (inicio) | probe+zstd-3 (espalhado) | regra manual | CART | logistica |
|---|---|---|---|---|---|---|---|
| texto | 0,00002 | 0,01963 | 0,00002 | 0,00002 | 0,00002 | 0,00123 | 0,00010 |
| codigo | 0,00451 | 0,01493 | 0,02778 | 0,00924 | 0,00678 | 0,00755 | 0,00477 |
| codigo_kernel | 0,00041 | 0,01873 | 0,00041 | 0,00041 | 0,00032 | 0,00053 | 0,00041 |
| binario | 0,00215 | 0,03309 | 0,02063 | 0,00653 | 0,12527 | 0,00796 | 0,00308 |
| cientifico | 0,04226 | 0,07267 | 0,04226 | 0,04226 | 0,02946 | 0,04425 | 0,00838 |
| banco_de_dados | 0,00000 | 0,00599 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |
| xml | 0,00117 | 0,01162 | 0,00117 | 0,00117 | 0,00117 | 0,00466 | 0,00079 |
| dna | 0,01501 | 0,00000 | 0,01501 | 0,01501 | 0,01501 | 0,00000 | 0,00000 |
| sint_logs | 0,01567 | 0,00000 | 0,01567 | 0,01567 | 0,01567 | 0,01567 | 0,01567 |
| sint_incompressivel | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |
| sint_ja_comprimido | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |

Granularidade 1M, regret de bytes (%) vs oraculo de tamanho do caminho sincrono, por classe:

| classe | zstd-3 fixo | zstd-1 fixo | probe+zstd-3 (inicio) | probe+zstd-3 (espalhado) | regra manual | CART | logistica |
|---|---|---|---|---|---|---|---|
| texto | 0,00 | 11,75 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 |
| codigo | 0,62 | 7,42 | 8,75 | 0,62 | 0,62 | 0,62 | 0,62 |
| codigo_kernel | 0,10 | 10,53 | 0,10 | 0,10 | 0,27 | 0,11 | 0,12 |
| binario | 0,00 | 9,00 | 4,46 | 2,32 | 22,52 | 0,62 | 0,00 |
| cientifico | 10,55 | 20,26 | 10,55 | 10,55 | 4,15 | 2,69 | 7,00 |
| banco_de_dados | 0,00 | 6,37 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 |
| xml | 0,00 | 7,44 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 |
| dna | 0,04 | 0,79 | 0,04 | 0,04 | 0,04 | 0,79 | 0,79 |
| sint_logs | 0,97 | 0,00 | 0,97 | 0,97 | 0,97 | 0,97 | 0,97 |
| sint_incompressivel | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 |
| sint_ja_comprimido | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 | 0,00 |
| GLOBAL | 1,12 | 7,63 | 1,91 | 1,41 | 3,35 | 0,47 | 0,80 |
| MEDIA DAS CLASSES | 1,12 | 6,69 | 2,26 | 1,33 | 2,60 | 0,53 | 0,86 |

Granularidade 1M, regret em US$/GB (nuvem+S3 Standard, camada morna; modelos treinados com o rotulo economico dessa celula):

| classe | zstd-3 fixo | zstd-1 fixo | probe+zstd-3 (inicio) | probe+zstd-3 (espalhado) | regra manual | CART | logistica |
|---|---|---|---|---|---|---|---|
| texto | 0,00000 | 0,04977 | 0,00000 | 0,00000 | 0,00000 | 0,00050 | 0,00000 |
| codigo | 0,00203 | 0,02444 | 0,02882 | 0,00203 | 0,00203 | 0,00093 | 0,00203 |
| codigo_kernel | 0,00027 | 0,02923 | 0,00027 | 0,00027 | 0,00075 | 0,00029 | 0,00051 |
| binario | 0,00000 | 0,04694 | 0,02328 | 0,01213 | 0,11756 | 0,00226 | 0,00000 |
| cientifico | 0,04032 | 0,07737 | 0,04032 | 0,04032 | 0,01586 | 0,00996 | 0,03396 |
| banco_de_dados | 0,00000 | 0,03085 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |
| xml | 0,00000 | 0,01254 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |
| dna | 0,00017 | 0,00327 | 0,00017 | 0,00017 | 0,00017 | 0,00327 | 0,00327 |
| sint_logs | 0,00326 | 0,00000 | 0,00326 | 0,00326 | 0,00326 | 0,00326 | 0,00326 |
| sint_incompressivel | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |
| sint_ja_comprimido | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 | 0,00000 |



## 9. Ganho do classificador

Granularidade 64K (ganho = bytes economizados em relacao a linha de base; US$/GB = valor liquido do modelo menos o da linha de base; modelo em LOFO; as colunas economicas usam o modelo treinado com o rotulo da coluna; a linha de 'medir todos' tem os mesmos bytes do oraculo e desconta o custo de comprimir todos os outros candidatos):

| modelo | contra | ganho de bytes | US$/GB nuvem|quente | US$/GB nuvem|morno | US$/GB nuvem|frio | US$/GB on-prem|quente | US$/GB on-prem|morno | US$/GB on-prem|frio |
|---|---|---|---|---|---|---|---|---|
| CART | zstd-3 fixo | 0,89% | -0,00129 | -0,00125 | -0,00126 | -0,00011 | -0,00011 | -0,00011 |
| CART | probe+zstd-3 (espalhado) | 0,96% | -0,00095 | -0,00092 | -0,00093 | -0,00009 | -0,00008 | -0,00008 |
| CART | medir todos | -0,62% | -0,00798 | -0,00799 | -0,00800 | -0,00065 | -0,00065 | -0,00065 |
| logistica | zstd-3 fixo | 1,11% | 0,00496 | 0,00498 | 0,00500 | 0,00043 | 0,00043 | 0,00043 |
| logistica | probe+zstd-3 (espalhado) | 1,18% | 0,00530 | 0,00532 | 0,00534 | 0,00046 | 0,00046 | 0,00046 |
| logistica | medir todos | -0,39% | -0,00173 | -0,00175 | -0,00174 | -0,00010 | -0,00011 | -0,00011 |
| regra manual | zstd-3 fixo | -1,49% | -0,00694 | -0,00689 | -0,00688 | -0,00062 | -0,00060 | -0,00060 |
| regra manual | probe+zstd-3 (espalhado) | -1,42% | -0,00661 | -0,00655 | -0,00655 | -0,00059 | -0,00057 | -0,00057 |
| regra manual | medir todos | -3,03% | -0,01363 | -0,01362 | -0,01362 | -0,00115 | -0,00114 | -0,00114 |

Tempo de compressao de 'medir todos' em multiplos do tempo de zstd-3 (vazoes medidas por classe, 1 nucleo): 8,0x so com os 9 candidatos sincronos, 173x com os 14 candidatos (inclui xz-9e, zstd-19 e bzip2-9).

Granularidade 256K (ganho = bytes economizados em relacao a linha de base; US$/GB = valor liquido do modelo menos o da linha de base; modelo em LOFO; as colunas economicas usam o modelo treinado com o rotulo da coluna; a linha de 'medir todos' tem os mesmos bytes do oraculo e desconta o custo de comprimir todos os outros candidatos):

| modelo | contra | ganho de bytes | US$/GB nuvem|quente | US$/GB nuvem|morno | US$/GB nuvem|frio | US$/GB on-prem|quente | US$/GB on-prem|morno | US$/GB on-prem|frio |
|---|---|---|---|---|---|---|---|---|
| CART | zstd-3 fixo | -0,08% | -0,00106 | -0,00100 | -0,00099 | -0,00010 | -0,00009 | -0,00009 |
| CART | probe+zstd-3 (espalhado) | 0,06% | -0,00044 | -0,00038 | -0,00038 | -0,00005 | -0,00003 | -0,00003 |
| CART | medir todos | -1,66% | -0,00771 | -0,00769 | -0,00768 | -0,00063 | -0,00063 | -0,00063 |
| logistica | zstd-3 fixo | 0,81% | 0,00349 | 0,00351 | 0,00351 | 0,00031 | 0,00031 | 0,00031 |
| logistica | probe+zstd-3 (espalhado) | 0,95% | 0,00410 | 0,00412 | 0,00412 | 0,00036 | 0,00036 | 0,00036 |
| logistica | medir todos | -0,76% | -0,00316 | -0,00318 | -0,00318 | -0,00022 | -0,00024 | -0,00024 |
| regra manual | zstd-3 fixo | -2,49% | -0,01107 | -0,01102 | -0,01101 | -0,00097 | -0,00096 | -0,00096 |
| regra manual | probe+zstd-3 (espalhado) | -2,35% | -0,01046 | -0,01040 | -0,01040 | -0,00092 | -0,00091 | -0,00090 |
| regra manual | medir todos | -4,11% | -0,01772 | -0,01771 | -0,01770 | -0,00150 | -0,00150 | -0,00150 |

Tempo de compressao de 'medir todos' em multiplos do tempo de zstd-3 (vazoes medidas por classe, 1 nucleo): 8,0x so com os 9 candidatos sincronos, 173x com os 14 candidatos (inclui xz-9e, zstd-19 e bzip2-9).

Granularidade 1M (ganho = bytes economizados em relacao a linha de base; US$/GB = valor liquido do modelo menos o da linha de base; modelo em LOFO; as colunas economicas usam o modelo treinado com o rotulo da coluna; a linha de 'medir todos' tem os mesmos bytes do oraculo e desconta o custo de comprimir todos os outros candidatos):

| modelo | contra | ganho de bytes | US$/GB nuvem|quente | US$/GB nuvem|morno | US$/GB nuvem|frio | US$/GB on-prem|quente | US$/GB on-prem|morno | US$/GB on-prem|frio |
|---|---|---|---|---|---|---|---|---|
| CART | zstd-3 fixo | 0,64% | 0,00254 | 0,00285 | 0,00255 | 0,00025 | 0,00022 | 0,00025 |
| CART | probe+zstd-3 (espalhado) | 0,92% | 0,00375 | 0,00406 | 0,00376 | 0,00035 | 0,00033 | 0,00035 |
| CART | medir todos | -0,47% | -0,00197 | -0,00169 | -0,00199 | -0,00010 | -0,00013 | -0,00011 |
| logistica | zstd-3 fixo | 0,31% | 0,00053 | 0,00053 | 0,00053 | 0,00005 | 0,00005 | 0,00005 |
| logistica | probe+zstd-3 (espalhado) | 0,60% | 0,00173 | 0,00174 | 0,00174 | 0,00015 | 0,00015 | 0,00015 |
| logistica | medir todos | -0,80% | -0,00398 | -0,00401 | -0,00401 | -0,00030 | -0,00031 | -0,00031 |
| regra manual | zstd-3 fixo | -2,20% | -0,00947 | -0,00942 | -0,00941 | -0,00083 | -0,00082 | -0,00082 |
| regra manual | probe+zstd-3 (espalhado) | -1,91% | -0,00826 | -0,00821 | -0,00820 | -0,00073 | -0,00071 | -0,00071 |
| regra manual | medir todos | -3,35% | -0,01397 | -0,01395 | -0,01395 | -0,00118 | -0,00117 | -0,00117 |

Tempo de compressao de 'medir todos' em multiplos do tempo de zstd-3 (vazoes medidas por classe, 1 nucleo): 8,0x so com os 9 candidatos sincronos, 172x com os 14 candidatos (inclui xz-9e, zstd-19 e bzip2-9).



## 10. Custo de features e inferencia

| gran. | blocos | zstd-3 (us/bloco) | features completas (us) | features do CART (us) | features da regra (us) | probe (us) | inferencia CART (us) | inferencia logistica (us) | CART: feat+inf / zstd-3 | logistica: feat+inf / zstd-3 | regra / zstd-3 | probe / zstd-3 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| 64K | 8782 | 121,4 | 147,4 | 109,6 (16 feat.) | 69,0 | 12,2 | 0,35 | 2,79 | 90,5% | 123,6% | 56,8% | 10,1% |
| 256K | 2207 | 516,3 | 158,2 | 123,2 (16 feat.) | 71,6 | 12,7 | 0,37 | 2,98 | 23,9% | 31,2% | 13,9% | 2,5% |
| 1M | 562 | 1911,8 | 157,8 | 60,4 (8 feat.) | 70,0 | 12,3 | 0,34 | 2,90 | 3,2% | 8,4% | 3,7% | 0,6% |

Estimativa para implementacao nativa (INFERENCIA PROPRIA, nao medida): cada feature distinta custa 1 passada sobre a amostra a 2000 B/us (~2 GB/s, histograma escalar), mais o probe zstd-1 medido; passadas do CART: 13, da logistica (21 features): 16, da regra: 7.

| gran. | CART nativo (us) | logistica nativo (us) | regra nativa (us) | CART nativo / zstd-3 | logistica nativa / zstd-3 |
|---|---|---|---|---|---|
| 64K | 36,8 | 45,0 | 14,3 | 30,3% | 37,0% |
| 256K | 39,4 | 45,5 | 14,3 | 7,6% | 8,8% |
| 1M | 26,6 | 45,1 | 14,3 | 1,4% | 2,4% |

Por classe (granularidade 1M), custo do CART (features do CART + inferencia) sobre o tempo do zstd-3 do mesmo bloco:

| classe | blocos | zstd-3 (us/bloco) | features do CART (us/bloco) | CART feat+inf / zstd-3 |
|---|---|---|---|---|
| texto | 64 | 2819,4 | 60,1 | 2,1% |
| codigo | 21 | 1715,2 | 59,9 | 3,5% |
| codigo_kernel | 239 | 1843,6 | 60,6 | 3,3% |
| binario | 55 | 2297,2 | 64,9 | 2,8% |
| cientifico | 58 | 1884,9 | 60,0 | 3,2% |
| banco_de_dados | 10 | 2014,5 | 63,7 | 3,2% |
| xml | 6 | 972,6 | 57,0 | 5,9% |
| dna | 5 | 2280,3 | 58,8 | 2,6% |
| sint_logs | 64 | 2157,8 | 58,6 | 2,7% |
| sint_incompressivel | 32 | 153,4 | 56,9 | 37,3% |
| sint_ja_comprimido | 8 | 162,9 | 57,6 | 35,5% |


## 11. Onde o modelo erra

Granularidade 64K, os 8 arquivos onde o CART (LOFO) mais gasta bytes alem do oraculo:

| arquivo | blocos | bytes extra (KB) | regret | acerto estrito |
|---|---|---|---|---|
| mozilla | 782 | 352,1 | 1,77% | 78% |
| mr | 153 | 211,1 | 6,76% | 46% |
| logs.log | 1024 | 183,5 | 1,08% | 0% |
| kernel/kernel_drivers_accel | 209 | 142,8 | 10,77% | 50% |
| samba | 330 | 137,9 | 2,40% | 79% |
| ooffice | 94 | 48,2 | 1,46% | 90% |
| kernel/kernel_crypto | 55 | 40,8 | 4,51% | 58% |
| kernel/kernel_Documentation__resto | 537 | 19,1 | 0,18% | 96% |

Granularidade 256K, os 8 arquivos onde o CART (LOFO) mais gasta bytes alem do oraculo:

| arquivo | blocos | bytes extra (KB) | regret | acerto estrito |
|---|---|---|---|---|
| x-ray | 33 | 1201,8 | 24,89% | 0% |
| logs.log | 256 | 761,8 | 4,58% | 0% |
| mozilla | 196 | 478,4 | 2,52% | 78% |
| mr | 39 | 320,1 | 9,83% | 10% |
| samba | 83 | 148,0 | 2,75% | 76% |
| ooffice | 24 | 32,9 | 1,05% | 88% |
| bible.txt | 16 | 22,6 | 1,80% | 62% |
| kernel/kernel_drivers_accel | 53 | 20,5 | 1,73% | 64% |

Granularidade 1M, os 8 arquivos onde o CART (LOFO) mais gasta bytes alem do oraculo:

| arquivo | blocos | bytes extra (KB) | regret | acerto estrito |
|---|---|---|---|---|
| mr | 10 | 356,8 | 11,16% | 0% |
| logs.log | 64 | 158,6 | 0,97% | 0% |
| mozilla | 49 | 135,0 | 0,73% | 96% |
| x-ray | 9 | 69,0 | 1,44% | 11% |
| kernel/kernel_crypto | 4 | 35,8 | 4,32% | 50% |
| samba | 21 | 31,8 | 0,62% | 90% |
| kernel/kernel_drivers_accel | 14 | 16,0 | 1,43% | 57% |
| nci | 32 | 15,1 | 0,53% | 34% |


Granularidade 64K, matriz de confusao de CART LOFO (linhas = plano do oraculo de tamanho; colunas = plano escolhido):

| oraculo \ escolhido | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|
| stored | 632 | . | 18 | 3 | . | . | . | . | . |
| z1 | 1 | 443 | 1158 | . | . | . | . | 38 | . |
| z3 | 7 | 199 | 5758 | 15 | . | . | . | 83 | . |
| d1 | 2 | . | 17 | . | . | . | . | . | . |
| d2 | . | . | 2 | . | 71 | . | . | 21 | . |
| d8 | . | . | 4 | . | . | . | . | 2 | . |
| b4 | . | 53 | 57 | . | 33 | . | . | 132 | . |
| b8 | . | 2 | 28 | . | 1 | . | . | 2 | . |

Granularidade 64K, matriz de confusao de probe+zstd-3 (espalhado) (linhas = plano do oraculo de tamanho; colunas = plano escolhido):

| oraculo \ escolhido | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|
| stored | 653 | . | . | . | . | . | . | . | . |
| z1 | 1 | . | 1639 | . | . | . | . | . | . |
| z3 | 25 | . | 6037 | . | . | . | . | . | . |
| d1 | 8 | . | 11 | . | . | . | . | . | . |
| d2 | . | . | 94 | . | . | . | . | . | . |
| d8 | . | . | 6 | . | . | . | . | . | . |
| b4 | . | . | 275 | . | . | . | . | . | . |
| b8 | . | . | 33 | . | . | . | . | . | . |

Granularidade 64K, matriz de confusao de regra manual (linhas = plano do oraculo de tamanho; colunas = plano escolhido):

| oraculo \ escolhido | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|
| stored | 653 | . | . | . | . | . | . | . | . |
| z1 | 1 | . | 1598 | . | . | . | . | . | 41 |
| z3 | 25 | . | 5370 | . | . | . | 2 | . | 665 |
| d1 | 1 | . | 16 | 2 | . | . | . | . | . |
| d2 | . | . | . | . | 34 | . | 1 | . | 59 |
| d8 | . | . | 1 | . | . | . | 2 | . | 3 |
| b4 | . | . | 35 | . | 113 | . | . | . | 127 |
| b8 | . | . | 1 | . | . | . | 1 | . | 31 |

Granularidade 256K, matriz de confusao de CART LOFO (linhas = plano do oraculo de tamanho; colunas = plano escolhido):

| oraculo \ escolhido | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|
| stored | 123 | . | 6 | 31 | . | . | . | . | . |
| z1 | . | 155 | 278 | . | . | . | . | 3 | . |
| z3 | 1 | 32 | 1448 | . | 2 | . | . | 28 | . |
| d1 | . | . | 4 | . | . | . | . | . | . |
| d2 | . | 14 | . | . | . | . | . | 14 | . |
| d8 | . | . | 1 | . | 1 | . | . | . | . |
| b4 | . | 12 | 34 | . | 5 | . | . | 9 | . |
| b8 | . | 1 | 5 | . | . | . | . | . | . |

Granularidade 256K, matriz de confusao de probe+zstd-3 (espalhado) (linhas = plano do oraculo de tamanho; colunas = plano escolhido):

| oraculo \ escolhido | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|
| stored | 160 | . | . | . | . | . | . | . | . |
| z1 | . | . | 436 | . | . | . | . | . | . |
| z3 | 6 | . | 1505 | . | . | . | . | . | . |
| d1 | 2 | . | 2 | . | . | . | . | . | . |
| d2 | . | . | 28 | . | . | . | . | . | . |
| d8 | . | . | 2 | . | . | . | . | . | . |
| b4 | . | . | 60 | . | . | . | . | . | . |
| b8 | . | . | 6 | . | . | . | . | . | . |

Granularidade 256K, matriz de confusao de regra manual (linhas = plano do oraculo de tamanho; colunas = plano escolhido):

| oraculo \ escolhido | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|
| stored | 160 | . | . | . | . | . | . | . | . |
| z1 | . | . | 429 | . | . | . | . | . | 7 |
| z3 | 8 | . | 1319 | . | . | . | . | . | 184 |
| d1 | 1 | . | 3 | . | . | . | . | . | . |
| d2 | . | . | . | . | 14 | . | . | . | 14 |
| d8 | . | . | . | . | . | . | . | . | 2 |
| b4 | . | . | 10 | . | 24 | . | . | . | 26 |
| b8 | . | . | . | . | . | . | . | . | 6 |

Granularidade 1M, matriz de confusao de CART LOFO (linhas = plano do oraculo de tamanho; colunas = plano escolhido):

| oraculo \ escolhido | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|
| stored | 5 | . | 32 | . | . | . | . | 3 | . |
| z1 | . | 11 | 88 | . | . | . | . | . | . |
| z3 | . | 9 | 391 | . | 1 | . | . | 1 | . |
| d2 | . | . | 7 | . | 1 | . | . | . | . |
| b4 | . | 1 | 4 | . | 8 | . | . | . | . |

Granularidade 1M, matriz de confusao de probe+zstd-3 (espalhado) (linhas = plano do oraculo de tamanho; colunas = plano escolhido):

| oraculo \ escolhido | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|
| stored | 40 | . | . | . | . | . | . | . | . |
| z1 | . | . | 99 | . | . | . | . | . | . |
| z3 | 3 | . | 399 | . | . | . | . | . | . |
| d2 | . | . | 8 | . | . | . | . | . | . |
| b4 | . | . | 13 | . | . | . | . | . | . |

Granularidade 1M, matriz de confusao de regra manual (linhas = plano do oraculo de tamanho; colunas = plano escolhido):

| oraculo \ escolhido | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|
| stored | 40 | . | . | . | . | . | . | . | . |
| z1 | . | . | 98 | . | . | . | . | . | 1 |
| z3 | 3 | . | 347 | . | . | . | . | . | 52 |
| d2 | . | . | 2 | . | 3 | . | . | . | 3 |
| b4 | . | . | 2 | . | 7 | . | . | . | 4 |



## 12. Generalizacao a classes novas (leave-one-class-out)

Granularidade 64K, regret de bytes (%) em leave-one-file-out contra leave-one-class-out:

| classe | CART LOFO | CART LOCO | logistica LOFO | logistica LOCO |
|---|---|---|---|---|
| texto | 0,00 | 0,04 | 0,01 | 0,01 |
| codigo | 2,40 | 2,40 | 1,41 | 1,41 |
| codigo_kernel | 0,40 | 0,20 | 0,15 | 0,68 |
| binario | 1,73 | 2,39 | 0,77 | 0,76 |
| cientifico | 1,35 | 3,24 | 1,25 | 1,39 |
| banco_de_dados | 0,00 | 0,00 | 0,00 | 0,00 |
| xml | 0,10 | 0,10 | 0,78 | 0,78 |
| dna | 0,00 | 0,00 | 0,00 | 0,00 |
| sint_logs | 1,08 | 1,08 | 1,08 | 1,08 |
| sint_incompressivel | 0,00 | 0,00 | 0,00 | 0,00 |
| sint_ja_comprimido | 0,00 | 0,00 | 0,00 | 0,00 |
| GLOBAL | 0,62 | 0,82 | 0,39 | 0,56 |

Granularidade 256K, regret de bytes (%) em leave-one-file-out contra leave-one-class-out:

| classe | CART LOFO | CART LOCO | logistica LOFO | logistica LOCO |
|---|---|---|---|---|
| texto | 0,15 | 1,09 | 0,01 | 0,01 |
| codigo | 2,75 | 2,75 | 1,75 | 1,75 |
| codigo_kernel | 0,06 | 0,06 | 0,11 | 0,76 |
| binario | 2,31 | 14,81 | 0,45 | 0,42 |
| cientifico | 9,20 | 5,01 | 2,18 | 3,34 |
| banco_de_dados | 0,00 | 0,00 | 0,00 | 0,00 |
| xml | 0,79 | 0,79 | 0,33 | 0,33 |
| dna | 0,00 | 0,00 | 0,00 | 0,00 |
| sint_logs | 4,58 | 4,58 | 4,58 | 4,58 |
| sint_incompressivel | 0,00 | 0,00 | 0,00 | 0,00 |
| sint_ja_comprimido | 0,00 | 0,00 | 0,00 | 0,00 |
| GLOBAL | 1,66 | 2,92 | 0,76 | 1,05 |

Granularidade 1M, regret de bytes (%) em leave-one-file-out contra leave-one-class-out:

| classe | CART LOFO | CART LOCO | logistica LOFO | logistica LOCO |
|---|---|---|---|---|
| texto | 0,00 | 0,83 | 0,00 | 0,00 |
| codigo | 0,62 | 0,62 | 0,62 | 0,62 |
| codigo_kernel | 0,11 | 0,06 | 0,12 | 0,98 |
| binario | 0,62 | 0,62 | 0,00 | 0,00 |
| cientifico | 2,69 | 19,27 | 7,00 | 14,29 |
| banco_de_dados | 0,00 | 0,00 | 0,00 | 0,00 |
| xml | 0,00 | 0,00 | 0,00 | 0,00 |
| dna | 0,79 | 0,79 | 0,79 | 0,79 |
| sint_logs | 0,97 | 0,97 | 0,97 | 0,97 |
| sint_incompressivel | 0,00 | 0,00 | 0,00 | 0,00 |
| sint_ja_comprimido | 0,00 | 0,00 | 0,00 | 0,00 |
| GLOBAL | 0,47 | 2,10 | 0,80 | 1,72 |



## 13. Transforms

Vazao dos transforms em numpy (CPU 0, buffer de 1 MiB aleatorio, mediana de 31 repeticoes) e estimativa nativa (INFERENCIA PROPRIA):

| transform | aplicar (MB/s) | inverter (MB/s) | estimativa nativa aplicar/inverter (MB/s) |
|---|---|---|---|
| d1 | 19539 | 731 | 4000 |
| d2 | 19457 | 1424 | 4000 |
| d4 | 19399 | 2731 | 4000 |
| d8 | 19477 | 5136 | 4000 |
| b4 | 4869 | 1772 | 3000 |
| b8 | 4675 | 3353 | 3000 |

Granularidade 64K por classe: ganho do melhor transform ESCOLHIDO POR BLOCO (min entre zstd-3 e os 6 transforms+zstd-3) sobre zstd-3, fracao de blocos em que algum transform vence zstd-3 por >= 3%, melhor transform fixo para a classe inteira, vazao comp./decomp. em MB/s (numpy) de zstd-3 -> melhor transform fixo+zstd-3, e ganho assincrono de T+xz-9e e T+zstd-19 sobre xz-9e e zstd-19 nos blocos em que T foi tentado:

| classe | ganho por bloco vs zstd-3 | blocos com transform >= 3% | melhor transform fixo (ganho) | comp MB/s | decomp MB/s | async T+xz / T+z19 vs sem T |
|---|---|---|---|---|---|---|
| texto | 0,00% | 0,1% | d1 -26,2% | 325 -> 320 | 1632 -> 505 | -40,6% / -14,1% (1 blocos) |
| codigo | 1,23% | 7,9% | d1 -18,1% | 517 -> 504 | 2270 -> 553 | -12,2% / 2,4% (26 blocos) |
| codigo_kernel | 0,12% | 0,9% | d1 -22,7% | 517 -> 504 | 2270 -> 553 | 8,5% / 15,6% (35 blocos) |
| binario | 0,84% | 6,5% | b4 -17,7% | 411 -> 379 | 1524 -> 819 | 8,5% / 14,4% (57 blocos) |
| cientifico | 11,57% | 31,2% | b4 -1,0% | 348 -> 325 | 1412 -> 786 | 7,3% / 15,6% (283 blocos) |
| banco_de_dados | 0,00% | 0,0% | d1 -17,0% | 453 -> 443 | 1817 -> 521 | - |
| xml | 0,00% | 0,0% | d1 -20,9% | 805 -> 773 | 2402 -> 560 | - |
| dna | 0,00% | 0,0% | b4 -2,3% | 375 -> 348 | 1576 -> 834 | - |
| sint_logs | 0,00% | 0,0% | d1 -24,7% | 446 -> 436 | 1764 -> 517 | - |
| sint_incompressivel | 0,00% | 0,0% | d1 0,0% | 5539 -> 4315 | 8369 -> 672 | - |
| sint_ja_comprimido | 0,00% | 0,0% | d1 0,0% | 5126 -> 4061 | 7929 -> 669 | - |

Granularidade 256K por classe: ganho do melhor transform ESCOLHIDO POR BLOCO (min entre zstd-3 e os 6 transforms+zstd-3) sobre zstd-3, fracao de blocos em que algum transform vence zstd-3 por >= 3%, melhor transform fixo para a classe inteira, vazao comp./decomp. em MB/s (numpy) de zstd-3 -> melhor transform fixo+zstd-3, e ganho assincrono de T+xz-9e e T+zstd-19 sobre xz-9e e zstd-19 nos blocos em que T foi tentado:

| classe | ganho por bloco vs zstd-3 | blocos com transform >= 3% | melhor transform fixo (ganho) | comp MB/s | decomp MB/s | async T+xz / T+z19 vs sem T |
|---|---|---|---|---|---|---|
| texto | 0,00% | 0,4% | d1 -17,4% | 325 -> 320 | 1632 -> 505 | -18,9% / -20,9% (1 blocos) |
| codigo | 0,79% | 6,0% | d1 -14,9% | 517 -> 504 | 2270 -> 553 | -16,7% / -11,1% (5 blocos) |
| codigo_kernel | 0,12% | 1,0% | d1 -18,2% | 517 -> 504 | 2270 -> 553 | 5,8% / 8,7% (9 blocos) |
| binario | 0,38% | 4,5% | b4 -18,4% | 411 -> 379 | 1524 -> 819 | 3,0% / 7,6% (10 blocos) |
| cientifico | 9,38% | 31,6% | b4 -3,5% | 348 -> 325 | 1412 -> 786 | 6,2% / 13,5% (72 blocos) |
| banco_de_dados | 0,00% | 0,0% | d1 -14,4% | 453 -> 443 | 1817 -> 521 | - |
| xml | 0,00% | 0,0% | d1 -15,4% | 805 -> 773 | 2402 -> 560 | - |
| dna | 0,00% | 0,0% | b4 -2,7% | 375 -> 348 | 1576 -> 834 | - |
| sint_logs | 0,00% | 0,0% | d1 -16,3% | 446 -> 436 | 1764 -> 517 | - |
| sint_incompressivel | 0,00% | 0,0% | d1 0,0% | 5539 -> 4315 | 8369 -> 672 | - |
| sint_ja_comprimido | 0,00% | 0,0% | d1 0,0% | 5126 -> 4061 | 7929 -> 669 | - |

Granularidade 1M por classe: ganho do melhor transform ESCOLHIDO POR BLOCO (min entre zstd-3 e os 6 transforms+zstd-3) sobre zstd-3, fracao de blocos em que algum transform vence zstd-3 por >= 3%, melhor transform fixo para a classe inteira, vazao comp./decomp. em MB/s (numpy) de zstd-3 -> melhor transform fixo+zstd-3, e ganho assincrono de T+xz-9e e T+zstd-19 sobre xz-9e e zstd-19 nos blocos em que T foi tentado:

| classe | ganho por bloco vs zstd-3 | blocos com transform >= 3% | melhor transform fixo (ganho) | comp MB/s | decomp MB/s | async T+xz / T+z19 vs sem T |
|---|---|---|---|---|---|---|
| texto | 0,00% | 0,0% | d1 -17,5% | 325 -> 320 | 1632 -> 505 | - |
| codigo | 0,42% | 4,8% | d1 -14,1% | 517 -> 504 | 2270 -> 553 | -40,7% / -22,6% (1 blocos) |
| codigo_kernel | 0,03% | 0,4% | d1 -17,7% | 517 -> 504 | 2270 -> 553 | 3,8% / 0,5% (1 blocos) |
| binario | 0,00% | 0,0% | b4 -16,8% | 411 -> 379 | 1524 -> 819 | - |
| cientifico | 9,43% | 32,8% | d2 -2,8% | 348 -> 342 | 1412 -> 709 | 5,9% / 11,9% (19 blocos) |
| banco_de_dados | 0,00% | 0,0% | d1 -15,6% | 453 -> 443 | 1817 -> 521 | - |
| xml | 0,00% | 0,0% | d1 -15,3% | 805 -> 773 | 2402 -> 560 | - |
| dna | 0,00% | 0,0% | b4 -2,1% | 375 -> 348 | 1576 -> 834 | - |
| sint_logs | 0,00% | 0,0% | d1 -15,6% | 446 -> 436 | 1764 -> 517 | - |
| sint_incompressivel | 0,00% | 0,0% | d1 0,0% | 5539 -> 4315 | 8369 -> 672 | - |
| sint_ja_comprimido | 0,00% | 0,0% | d1 0,0% | 5126 -> 4061 | 7929 -> 669 | - |


## 14. Elegibilidade ao caminho sincrono (>= 290 MB/s por nucleo na compressao, granularidade 1 MiB)

| classe | stored | z1 | z3 | d1 | d2 | d4 | d8 | b4 | b8 |
|---|---|---|---|---|---|---|---|---|---|
| texto | 14400 | 451 | 325 | 320 | 320 | 320 | 320 | 305 | 304 |
| codigo | 12945 | 702 | 517 | 504 | 504 | 504 | 504 | 468 | 466 |
| codigo_kernel | 12945 | 702 | 517 | 504 | 504 | 504 | 504 | 468 | 466 |
| binario | 14129 | 679 | 411 | 403 | 403 | 403 | 403 | 379 | 378 |
| cientifico | 12936 | 737 | 348 | 342 | 342 | 342 | 342 | 325 | 324 |
| banco_de_dados | 13635 | 629 | 453 | 443 | 443 | 443 | 443 | 414 | 413 |
| xml | 15154 | 990 | 805 | 773 | 773 | 773 | 773 | 691 | 687 |
| dna | 15899 | 402 | 375 | 368 | 368 | 368 | 368 | 348 | 347 |
| sint_logs | 14155 | 566 | 446 | 436 | 436 | 436 | 436 | 408 | 407 |
| sint_incompressivel | 14955 | 5357 | 5539 | 4315 | 4311 | 4309 | 4312 | 2591 | 2535 |
| sint_ja_comprimido | 11391 | 6083 | 5126 | 4061 | 4057 | 4055 | 4058 | 2497 | 2445 |

(*) abaixo de 290 MB/s por nucleo na compressao (vazao do codec da classe medida em 1 MiB combinada em serie com o custo do transform em numpy).


## 15. Sensibilidade ao custo do transform (numpy medido contra nativo estimado), regret em US$/GB

| gran. | plano | numpy / nativo nuvem|quente | numpy / nativo nuvem|morno | numpy / nativo nuvem|frio | numpy / nativo on-prem|quente | numpy / nativo on-prem|morno | numpy / nativo on-prem|frio |
|---|---|---|---|---|---|---|---|
| 64K | zstd-3 fixo | 0,00689 / 0,00690 | 0,00693 / 0,00693 | 0,00693 / 0,00693 | 0,00059 / 0,00060 | 0,00060 / 0,00060 | 0,00060 / 0,00060 |
| 64K | cart | 0,00817 / 0,00815 | 0,00818 / 0,00819 | 0,00819 / 0,00817 | 0,00070 / 0,00071 | 0,00071 / 0,00071 | 0,00071 / 0,00071 |
| 64K | logit | 0,00192 / 0,00193 | 0,00194 / 0,00194 | 0,00193 / 0,00193 | 0,00016 / 0,00016 | 0,00017 / 0,00017 | 0,00017 / 0,00017 |
| 256K | zstd-3 fixo | 0,00684 / 0,00686 | 0,00688 / 0,00688 | 0,00688 / 0,00688 | 0,00059 / 0,00059 | 0,00060 / 0,00060 | 0,00060 / 0,00060 |
| 256K | cart | 0,00790 / 0,00786 | 0,00788 / 0,00787 | 0,00787 / 0,00776 | 0,00069 / 0,00068 | 0,00068 / 0,00069 | 0,00069 / 0,00068 |
| 256K | logit | 0,00335 / 0,00335 | 0,00337 / 0,00337 | 0,00337 / 0,00337 | 0,00028 / 0,00028 | 0,00029 / 0,00029 | 0,00029 / 0,00029 |
| 1M | zstd-3 fixo | 0,00470 / 0,00471 | 0,00473 / 0,00473 | 0,00473 / 0,00473 | 0,00040 / 0,00041 | 0,00041 / 0,00041 | 0,00041 / 0,00041 |
| 1M | cart | 0,00216 / 0,00216 | 0,00188 / 0,00188 | 0,00218 / 0,00218 | 0,00015 / 0,00016 | 0,00019 / 0,00019 | 0,00016 / 0,00019 |
| 1M | logit | 0,00417 / 0,00418 | 0,00420 / 0,00420 | 0,00420 / 0,00420 | 0,00036 / 0,00036 | 0,00036 / 0,00037 | 0,00037 / 0,00037 |


Conferencia interna: diferenca maxima absoluta entre o valor liquido vetorizado deste programa e `economia.Contexto.liquido_gb` em 200 pares (bloco, candidato) sorteados por granularidade: 0.00e+00 US$/GB.
