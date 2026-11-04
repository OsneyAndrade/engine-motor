# Segurança do motor: modelo de ameaça, defesas e lacunas

Escrito em 2026-11-04 (marco M2b do [plano de trabalho](plano-de-trabalho.md)). Cada afirmação aponta para o código que a sustenta e para o teste que a exercita; o que **não** foi medido está dito como tal, e a Seção 6 lista o que o motor ainda não cobre. Os números de teste vêm de `cargo test --locked` em contêiner (motor 0.3.0) e de `scripts/smoke.sh`; como rodá-los: [testes.md](testes.md) e [uso.md](uso.md#1-verificação-rápida).

## 1. Premissa

O motor é uma API HTTP que recebe, de clientes que **não são confiáveis**, três tipos de bytes: o conteúdo a comprimir (`/compress`, `/analyze`), o **envelope** `.syntra` a reconstruir (`/decompress`, `/inspect`) e JSON de administração. O envelope é a entrada mais perigosa: ele diz ao motor qual codec usar, quais transforms desfazer, quantos bytes esperar e qual hash conferir, e **tudo isso vem do cliente**. O checksum do envelope (`src/container.rs`, `payload_checksum`: 8 bytes do BLAKE3 de `data`) detecta corrupção acidental de `data`; **não autentica** nada: não cobre `codec`, `transforms`, `original_size`, `file_hash` nem os nomes, é conferido só quando diferente de zero (um envelope sem o campo passa sem conferência) e quem forja o envelope pode recalculá-lo (os testes de bomba o fazem com `container::seal`). Por isso toda defesa abaixo parte de "o envelope é hostil".

Fora de escopo deste documento: TLS, rede e isolamento de contêiner (Seção 6).

## 2. Modelo de ameaça

| Ameaça | Como se manifesta | Defesa | Onde | Teste |
|---|---|---|---|---|
| **Envelope forjado** | Cliente envia `/decompress` com campos escolhidos por ele: varint colunar de 2^64-1, cadeia de transforms de 9 passos, `original_size` absurdo, codec inexistente, hash de dicionário trocado, `file_hash` gigante | O restauro nunca confia no tamanho declarado para alocar; toda aritmética sobre comprimentos é verificada; erro tipado (`4xx` ou `422`) em vez de pânico. Com `panic = "abort"` um pânico derrubaria o processo todo | `transform::csv_invert` (soma com `checked_add`, dimensões ≤ 32 Mi campos e ≤ bytes do payload), `transform::invert_chain_limited` (≤ 8 passos, teto por estágio de `n + n/8 + 4096`), `container::restore_with_limit`, `dict_store::is_valid_id` e conferência do digest | `robustez_tests` (fuzz determinístico, Seção 5), `envelope_forjado_com_bomba_vira_422_sem_derrubar_o_servico`, smoke `csv-hostil` |
| **Bomba de descompressão** | Payload de poucos KB que expande para GB (1 GiB de zeros cabe em 6,5 KB de bzip2 ou 32,8 KB de zstd) | Teto de saída em **todos** os codecs: o menor entre `original_size` do envelope e `SYNTRA_MAX_DECOMPRESSED_MB` (`Config::max_decompressed_bytes`). Estouro devolve `422 decompressed_too_large` com `hint` | `codec::decode_limited`, `planner::restore_limited`, `container::restore_with_limit`, mapeamento em `service::reconstruct` | `bombas_tests` (bombas reais de 1 GiB em 6 codecs), e2e `bombas_reais_de_1_gib_em_todos_os_codecs_viram_422_rapido_pela_api`, smoke (xz, bzip2, deflate, lz4) |
| **Corpo gigante** | Upload acima do que a máquina aguenta | `DefaultBodyLimit` = `SYNTRA_MAX_BODY_MB` em todas as rotas; o excesso vira `413` em **JSON** (`payload_too_large`, com `hint`), com `x-request-id` e CORS | `src/api/mod.rs` (`payload_too_large_as_json`), OpenAPI (`PayloadTooLarge`) | e2e `corpo_acima_do_limite_devolve_o_json_de_erro_padrao_em_todas_as_rotas_com_corpo` (11 rotas), smoke (contêiner com `SYNTRA_MAX_BODY_MB=1`) |
| **Id malicioso** | `:id` com `../`, tamanho errado, bytes não hexadecimais; id de dicionário com `/`; nome de arquivo com caminho; cabeçalhos longos | O id de objeto vira 32 bytes antes de qualquer caminho (`VaultManager::parse_id`, 64 hex); o caminho do vault é derivado só do hash; id de dicionário só aceita `[A-Za-z0-9._-]` até 128 bytes; o nome de arquivo do cliente só vira metadado (parâmetros ligados no SQL) e passa por `sanitize_filename` e `content_disposition`; `Idempotency-Key` ≤ 200 e `x-request-id` ≤ 128 caracteres ASCII imprimíveis | `vault.rs`, `dict_store.rs`, `api/dto.rs`, `api/mod.rs` | e2e `identificador_malicioso_e_rejeitado`, smoke (id curto, 63 hex, não hexadecimal: `400 invalid_object_id`), `vault::tests::parse_id_rejeita_travessia_de_caminho` |
| **Reingestão** | A pasta observada é reprocessada a cada reinício (referências e consumo faturável inflam); retry de cliente cobra duas vezes | Ingestão por diretório: registro persistido no banco (`watch_seen`: caminho, tamanho, mtime em ns, hash do objeto); depois de reiniciar, arquivo inalterado não é lido de novo, só mtime mudou com o mesmo conteúdo atualiza o registro sem nova referência, conteúdo novo vira objeto novo. API: `Idempotency-Key` devolve `replayed: true` sem novo evento | `src/watcher.rs` (`SeenStore`), `src/watch_registry.rs`, `service::compress` | `watcher::tests::reinicio_nao_reingere_arquivo_inalterado_mas_reingere_o_modificado`, `so_a_data_de_modificacao_mudar_nao_gera_nova_referencia`, smoke (1 referência e nenhum evento novo depois de `docker restart`), e2e de idempotência |
| **Consumo forjado** | Envelope com `file_hash` de 1 MiB faz o ledger gravar 2 MiB de texto, ou atribuir o evento a outro objeto | `/decompress` só registra o hash do envelope quando tem exatamente 32 bytes; fora disso grava vazio. O ledger não depende de nenhum valor do corpo para a fatura (`compress_bytes_*`) | `api/compress.rs` (`ledger_hash`) | e2e `hash_forjado_no_envelope_nao_entra_no_ledger` |

## 3. Limites configuráveis

| Variável | Padrão | Efeito | Observação |
|---|---|---|---|
| `SYNTRA_MAX_BODY_MB` | `4096` (`10240` no compose de produção) | Corpo máximo de qualquer requisição; acima: `413 payload_too_large` | O corpo é lido **inteiro em memória** (pico medido de ~3,5x o arquivo na compressão): uploads simultâneos somam |
| `SYNTRA_MAX_DECOMPRESSED_MB` | igual a `SYNTRA_MAX_BODY_MB` | Teto de saída de qualquer reconstrução (`/decompress`, `/objects/:id/content`, rotas legadas); acima: `422 decompressed_too_large` | Vale o **menor** entre este teto e o `original_size` do envelope; `original_size = 0` (não declarado) usa só este teto. Aparece em `GET /api/v1/stats` (`config.max_decompressed_mb`) |

Constantes sem variável: transforms por envelope ≤ 8, memória do decodificador xz ≤ 256 MiB (`XZ_MEMLIMIT`), `MAX_DECOMPRESSED` de 64 GiB como teto absoluto de `decode_limited`, 32 Mi campos por tabela colunar.

Como escolher o teto: o padrão (igual ao corpo) garante que tudo que o motor aceitou comprimir também pode ser reconstruído, mas **não protege** contra um cliente que gaste 4 GiB de RAM por requisição com um envelope de 150 KB (Seção 6). Se o maior objeto legítimo do cliente é de 500 MB, defina `SYNTRA_MAX_DECOMPRESSED_MB=512`. Cuidado com o inverso: `/compress` **não** confere o tamanho do original contra este teto; com o teto abaixo do corpo, um objeto grande é aceito e arquivado, mas `/content` devolve `422 decompressed_too_large` (o e2e `teto_de_descompressao_da_configuracao_devolve_422_com_codigo_estavel` mostra o caso; `/essence` ainda entrega o container).

## 4. Como cada codec respeita o teto

Implementação em `src/codec.rs` (`decode_limited`). Em todos, o buffer inicial é no máximo `min(tamanho declarado, teto, 64 x entrada + 1 MiB)` e cresce por dobra limitada ao teto; a saída nunca passa do teto.

| Codec | Mecanismo |
|---|---|
| `stored` | recusa antes de copiar se o payload já passa do teto |
| `lz4` | lê o prefixo de 4 bytes **antes** de alocar; prefixo acima do teto: `decompressed_too_large`; prefixo acima de `255 x corpo + 1 MiB` (impossível em LZ4 válido): `restore_failed`, sem alocar |
| `zstd` | se o frame declara o tamanho do conteúdo e ele passa do teto, recusa sem decodificar; senão decodifica em blocos de 128 KiB e para no teto (frame sem tamanho declarado também) |
| `brotli` | o destino é um `CappedSink` que recusa a escrita que passaria do teto |
| `xz` | blocos de 128 KiB com teto e `memlimit` de 256 MiB no decodificador |
| `bzip2`, `deflate` | blocos de 128 KiB com teto |

No caminho do formato v4 em blocos (`src/blockfile.rs`, ainda **não** ligado à API, é o M3) o teto de cada bloco é o `original_len` do próprio bloco (≤ 64 MiB, normalmente 1 MiB) mais a folga de `stored_len`; lá o erro é `block_decode_failed` e não `decompressed_too_large`. O `stored_len` máximo de um bloco (`original_len + 12,5% + 4 KiB`) já impede que `lz4`, `brotli` e `deflate` tragam uma bomba de 1 GiB: o escritor e o leitor recusam o payload antes de decodificar.

## 5. Evidência medida (2026-11-04)

Máquina de desenvolvimento, contêiner `rust:1.88-slim-bookworm`, perfil de teste (sem otimização), PRNG xorshift com sementes fixas (reproduzível). Os contadores `FUZZ ...` saem com `cargo test --locked robustez -- --nocapture`.

### 5.1 Fuzz determinístico no código atual

Contadores impressos pelos testes (`FUZZ ...`), execução limpa de `cargo test --locked -- --nocapture` (224 passam, 0 falham, 2 ignorados). "Pico" é o maior total de bytes **solicitados ao alocador do Rust** por caso (`src/memtrack.rs`).

| Teste | Casos | Pânicos | Saída acima do teto | Caso acima do limite de memória | Pico máx. | Caso mais lento |
|---|---:|---:|---:|---:|---:|---:|
| `delta` | 25.000 | 0 | 0 | 0 | 1.310 B | 0 ms |
| `byte_split` | 25.000 | 0 | 0 | 0 | 1.324 B | 0 ms |
| `rle` | 25.000 | 0 | 0 | 0 | 51.200 B | 0 ms |
| `csv_colunar/comprimentos` | 25.000 | 0 | 0 | 0 | 694 B | 0 ms |
| `csv_colunar/dimensoes` | 25.000 | 0 | 0 | 0 | 959 B | 0 ms |
| `csv_colunar/mutacoes` | 25.000 | 0 | 0 | 0 | 936 B | 3 ms |
| `codecs` (7 codecs x 3.000) | 21.000 | 0 | 0 | 0 | 17,9 MB | 5 ms |
| `envelopes_forjados` (checksum válido) | 20.000 | 0 | 0 | 0 | 19,0 MB | 70 ms |
| `envelopes_forjados/original_size` hostil | 20.000 | 0 | 0 | 0 | 18,0 MB | 4 ms |
| `corpus_dourado` (mutação de 61 envelopes v2/v3) | 24.400 | 0 | 0 | 0 | 17,9 MB | 9 ms |

No corpus dourado mutado, 17.609 dos 24.400 casos passaram do `parse` e chegaram ao restauro; 3.011 restauraram, e os demais pararam em erro tipado: `restore_failed` 8.106, `envelope_invalid` 3.561, `envelope_corrupted` 3.230, `fidelity_mismatch` 2.450, `envelope_malformed` 2.292, `decompressed_too_large` 1.747, `dictionary_unavailable` 2, `dictionary_mismatch` 1.

### 5.2 Bombas reais de 1 GiB no código atual

1 GiB de zeros comprimido de verdade em cada codec; recusa no teto de 64 MiB (uma execução, `cargo test bombas -- --nocapture`; os tempos variam de execução para execução):

| Codec | Tamanho da bomba | Recusa | Pico de alocação do Rust |
|---|---:|---:|---:|
| zstd (tamanho declarado no frame) | 32.790 B | 0 ms | 32 B |
| lz4 | 4.210.767 B | 0 ms | 32 B |
| brotli | 5.505.025 B | 917 ms | 71,3 MB |
| xz | 156.316 B | 269 ms | 67,2 MB |
| bzip2 | 6.533 B | 220 ms | 67,2 MB |
| deflate | 4.885.611 B | 64 ms | 67,3 MB |

O zstd **sem** tamanho declarado também é cortado no teto, e o teto exato passa enquanto um byte a menos falha, em todos os codecs. Pela API (`/decompress`, teto de 32 MiB, `original_size` 0, 1 GiB e 2^64-1) as seis bombas viram `422 decompressed_too_large` com `hint`, cada uma em menos de 10 s. No formato v4: zstd é recusado pelo tamanho declarado (0 ms, 242 B), xz e bzip2 são cortados no `original_len` do bloco (2 a 5 ms, cerca de 1,2 MB), e lz4, brotli e deflate nem entram no arquivo porque o payload de 1 GiB de zeros passa de `stored_len` máximo do bloco.

### 5.3 Os mesmos testes no código antigo (commit `a3b9ab0`)

Método: `git archive a3b9ab0` para um diretório fora do repositório, os arquivos de teste copiados e funções de cola que repassam `decode_limited`, `invert_limited` e `restore_with_limit` para as funções antigas **sem teto** (procedimento em [testes.md](testes.md#como-reproduzir-o-teste-falha-no-código-antigo)). Cada teste rodou isolado, num contêiner de 12 GiB.

| Defeito | Teste | Código antigo | Código atual |
|---|---|---|---|
| B6 | `fuzz_csv_colunar_comprimentos_hostis_nao_estouram_a_soma` | **2.989 de 25.000 casos com pânico** (`attempt to add with overflow`) | 0 |
| B6 | `fuzz_csv_colunar_dimensoes_hostis_nao_travam_nem_alocam` | caso travado por mais de 60 s; o vigia do teste abortou o processo (`VIGIA csv_colunar/dimensoes ... abortando`, SIGABRT) | 25.000 casos, 0 ms no pior |
| B6 | `fuzz_csv_colunar_mutacoes_do_formato_valido` e `fuzz_de_mutacao_do_corpus_dourado_nao_causa_panico` | processo abortado (SIGABRT) depois de "running for over 60 seconds" (a mensagem do vigia só confirmei em `dimensoes`) | 25.000 e 24.400 casos, 0 pânicos |
| B6/B7 | `fuzz_envelopes_forjados_com_checksum_valido` | 1.183 de 20.000 casos pediram mais memória que o limite do teste; pico de **4.295.100.579 B** | 0 casos, pico 19,0 MB |
| B7 | `fuzz_envelopes_forjados_com_tamanho_original_hostil` | 1.522 de 20.000 casos acima do limite; pico de **55.788.563.335 B** (141 s) | 0 casos, pico 18,0 MB (4 ms no pior) |
| B7 | `fuzz_dos_decoders_de_todos_os_codecs` | 1.374 de 21.000 casos acima do limite; pico de **58.029.081.276 B** (129 s) | 0 casos, pico 17,9 MB |
| B7 | `bomba_de_1_gib_em_cada_codec_e_recusada_no_teto_de_64_mib`, `zstd_sem_tamanho_declarado_tambem_respeita_o_teto` | `zstd: a bomba foi expandida em vez de recusada` | recusadas (tabela 5.2) |
| B7 | `envelope_forjado_com_bomba_devolve_decompressed_too_large` | o restauro expandiu o 1 GiB inteiro e só então reclamou: `tamanho reconstruído (1073741824) difere do declarado (1048576)` | `decompressed_too_large` com teto de 1 MiB |
| B7 | `original_size_forjado_nao_faz_o_motor_alocar_o_declarado` | `declarado 64424509440: alocou 64424640515 bytes para 10 bytes reais` | menos de 4 MiB |
| B7 | `amplificacao_por_rle_encadeado_e_barrada_a_cada_estagio` | expandiu 16 KiB declarados para 524.288 B antes de notar | recusado por estágio, menos de 2 MiB |

Leitura honesta dos números do código antigo: "pico" é o que foi **solicitado** ao alocador, quase todo memória virtual que ninguém tocou; o contêiner de 12 GiB não matou o processo porque o kernel aceitou as reservas. O que a medição prova é que o código antigo **pede** dezenas de GB por requisição a partir de um envelope de poucos bytes, e que os varints hostis o derrubam ou o travam. `fuzz_delta`, `fuzz_byte_split` e `fuzz_rle` também falham na cola antiga (283, 304 e 1.530 casos "acima do teto"), mas só porque a cola não tem teto algum: esses três transforms nunca entraram em pânico (0 pânicos), então **não** são evidência de defeito anterior.

Outras provas "antes e depois" feitas em cópia do código atual com **só uma correção desfeita**: sem o registro persistido da ingestão (cópia com `SeenStore::load` sem ler o banco), os testes `reinicio_nao_reingere_arquivo_inalterado_mas_reingere_o_modificado` e `so_a_data_de_modificacao_mudar_nao_gera_nova_referencia` falham (`reinício do watcher: sem nova referência: left 2, right 1`); sem a checagem de plausibilidade do prefixo LZ4, o envelope forjado de 68 bytes faz o restauro alocar **3.500.000.000 B**; e com o `savings_pct` do `/usage` calculado sobre todos os eventos, o e2e `ledger_registra_bytes_e_cpu_de_analyze_decompress_e_leitura` falha com `45.18` no lugar de `95.92` depois de um `analyze`, um `decompress` e duas leituras.


## 6. O que ainda não está coberto

Itens abertos, na ordem em que eu os trataria. Nenhum deles foi corrigido no M2b.

1. **O teto padrão é alto.** Com `SYNTRA_MAX_DECOMPRESSED_MB` igual ao corpo (4096), um envelope de 150 KB (xz) ou 6,5 KB (bzip2) pode fazer o motor alocar e escrever até 4 GiB por requisição, até o número de permits de CPU (`max_compress_permits`) em paralelo, gastando CPU proporcional (a recusa de 64 MiB levou de 64 ms a 917 ms conforme o codec; 4 GiB seriam 64 vezes isso, **não medido**). A amplificação é **limitada**, não eliminada. Reduza o teto ao maior objeto legítimo e mantenha a autenticação ligada.
2. **Sem limite de concorrência de upload, sem limite por IP ou por chave, sem timeout de leitura do corpo.** O semáforo de CPU só vale depois de o corpo estar em memória, e o código não configura timeout, limite de concorrência nem limite de taxa (a busca por `timeout`, `ConcurrencyLimit` e `RateLimit` em `src/main.rs`, `src/api/mod.rs` e `src/service.rs` não acha nada; não testei um cliente lento). N uploads grandes simultâneos custam N x corpo de RAM, e um cliente lento segura a conexão (*slowloris*). Use proxy reverso com `limit_req`, `client_body_timeout` e limite de conexões.
3. **O checksum do envelope não autentica** (Seção 1). Não há assinatura nem HMAC do envelope: um envelope forjado e bem formado é restaurado como qualquer outro. Sem `file_hash` de 32 bytes a reconstrução não é conferida (`x-syntra-fidelity-checked: false`).
4. **Formato v4 (ainda fora da API):** o **rodapé é único e sem redundância** (questão O2 do ADR): corrupção é detectada, não reparada, e o objeto fica ilegível. O cabeçalho de 7 bytes não tem hash próprio (flags e log2 são repetidos no rodapé). Detalhes em [blockfile-v4.md](blockfile-v4.md).
5. **`blockfile::Reader::restore_to` entrega bytes ao destino antes de conferir o hash do arquivo** (o `fidelity_mismatch` chega depois de os bytes saírem). `restore_all` descarta tudo nesse caso. Quem ligar o v4 à API no M3 deve gravar em área temporária e promover só em caso de `Ok`.
6. **Sem TLS no motor.** HTTP puro; o `x-api-key` trafega em claro sem proxy com TLS. CORS fica liberado para qualquer origem sem `SYNTRA_CORS_ORIGINS`.
7. **Rotas abertas por desenho:** `/health`, `/ready`, `/metrics`, `/api/v1/openapi.json` e o dashboard não exigem chave. Sem `SYNTRA_API_KEYS` e sem chave emitida, **toda** a API é aberta (o log avisa).
8. **`/compress` não confere `SYNTRA_MAX_DECOMPRESSED_MB`** (Seção 3): um teto abaixo do corpo cria objetos que o motor aceita e depois recusa reconstruir.
9. **Sem cota de armazenamento por tenant:** um tenant autenticado pode encher o vault.
10. **Memória fora do alcance do medidor de testes.** O `memtrack` dos testes só conta alocações do Rust; o `memlimit` do xz (256 MiB por decodificação concorrente, no pior caso `núcleos x 256 MiB`) e o buffer de janela do zstd vêm do C. O `decoder.window_log_max(31)` do zstd (`codec.rs`) aceita um quadro que declara janela de até 2 GiB: medi: um quadro zstd de **12 bytes** que declara janela de 2 GiB e conteúdo de 3 bytes faz o `ZSTD_sizeof_DCtx` ir de 95.992 B para 2.147.972.920 B (2048 MiB), com `VmSize` +2048 MiB e `VmRSS` +1,1 MiB, ou seja, reserva de endereço sem uso de memória residente; com `WindowLogMax(27)` o mesmo quadro é recusado (`Frame requires too much memory for decoding`). Medido num experimento descartável, não no motor. Com uma entrada de 13,4 MB, os frames que o próprio `codec::encode` produz nos níveis 3, 10, 15, 19 e 22 decodificam com `WindowLogMax(27)`, ou seja, o motor não precisa de janelas maiores para os próprios dados. Reservar 2 GiB de endereço por decodificação pode falhar sob `vm.overcommit_memory=2` ou `ulimit -v`; baixar o limite para 27 (e abrir 31 só por tenant) quebraria quadros longos que o plano T1.6 prevê, por isso **não** mudei
11. **Ingestão por diretório:** o registro é por caminho absoluto (renomear um arquivo é um arquivo novo: nova referência, sem novo blob); um arquivo trocado por outro de **mesmo tamanho e mesmo mtime** não é percebido; se o banco for perdido o registro se perde e o diretório é reingerido; a versão antiga de um arquivo modificado **mantém** sua referência (solte com `DELETE /objects/:id`); **medi** que ele segue links simbólicos: um `watch_in/link.txt -> /etc/hostname` criado dentro do contêiner foi arquivado em 2 s e `GET /objects/:id/content` devolveu o conteúdo de `/etc/hostname`. Quem pode escrever no diretório observado faz o motor arquivar qualquer arquivo legível pelo processo (uid 1000 do contêiner); não exponha esse diretório a quem não é confiável

## 7. Checklist de operação

1. Defina `SYNTRA_API_KEYS` (o compose de produção exige) e coloque proxy com TLS, limite de taxa e `client_body_timeout` na frente.
2. Defina `SYNTRA_MAX_BODY_MB` e `SYNTRA_MAX_DECOMPRESSED_MB` pelo **maior objeto legítimo**, não pelo padrão, e dimensione a RAM por `3,5 x corpo x requisições simultâneas` (e por `núcleos x 256 MiB` se o xz estiver em uso).
3. Publique a porta só em loopback ou na rede interna (o compose já faz: `SYNTRA_PUBLISH_HOST`, padrão `127.0.0.1`).
4. Restrinja `/metrics` no proxy.
5. Faça backup do banco junto do vault: o registro da ingestão por diretório e o ledger vivem nele.
6. Alerte em `syntra_verify_failures_total > 0` e em picos de `422 decompressed_too_large` (indício de envelope forjado).
