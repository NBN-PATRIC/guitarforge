# GuitarForge: estúdio virtual de guitarra

Amp digital completo, offline, sem conta, open-source (MIT). Cabo Rocksmith USB direto no PC.
Um único executável: liga o cabo, abre o app, escolhe o som, toca.

## Estado (31/10/2026, v0.3)

Interface amigável pronta e testada com o cabo real. A fase "personalizável" fica para depois (lista no fim).
- Rodar: `cargo run --release`; executável em `target\release\guitarforge.exe`.
- Ferramentas de diagnóstico (build debug, que tem console):
  - `--dispositivos`: entradas e saídas com o formato de cada uma.
  - `--analisar arquivo.wav [fmin] [Hz da corda]`: roda o afinador sobre uma gravação (a cada 50 ms).
  - `--espectro arquivo.wav inicio fim`: picos do espectro de 40 a 700 Hz (diz qual nota soava de fato).
  - `--ruido arquivo.wav inicio fim`: pico e RMS cru e sem zumbido (o que o gate enxerga).
  - `--print arquivo.bmp [largura altura] [fim]`: abre a janela sem áudio e grava o que o próprio app desenhou
    (não captura a tela, que pegaria outras janelas); `fim` rola até embaixo. Converter para PNG com o
    System.Drawing do PowerShell para ver.
  - `--wasapi`: o que cada dispositivo aceita no exclusivo, período mínimo e o do `IAudioClient3`.
  - `--teste-motor [s] [entrada|saida|ambos|nenhum] [baixa|normal|segura] [saída]`: liga o motor **mudo**, espera
    1 s e mede o regime: atraso, falhas (anel vazio), descartes (anel cheio) e quanto a saída exclusiva pediu por
    segundo. Não precisa de guitarra: é o primeiro teste quando aparecer picote. Exclusivo toma o dispositivo de
    quem estiver usando (inclusive de outro GuitarForge aberto, que cai com "cabo desconectado?").
  - `--diagnostico s [entrada|saida|ambos|nenhum] [baixa|normal|segura] [1|2|4] pasta`: toca de verdade com o preset e
    os dispositivos salvos e grava `1-cabo.wav`, `2-cadeia.wav` e `3-enviado.wav` (o que foi ao dispositivo), com
    tempos de bloco, falhas, descartes e a comparação cadeia em tempo real x offline e enviado x cadeia. Começa a
    gravar na hora: combinar antes com quem vai tocar (31/10 três rodadas saíram sem guitarra).
- Estado, presets do usuário e log: `%APPDATA%\GuitarForge\` (`estado.json`, `presets\*.json`, `guitarforge.log`).
- Gravações: `%USERPROFILE%\Music\GuitarForge\GuitarForge NNN.wav` (processado, estéreo) e `... (DI).wav` (seco do
  cabo, mono).
- IRs carregados: cópia em `%APPDATA%\GuitarForge\irs\` (nome do arquivo e impressão digital do conteúdo); o preset
  guarda o nome da cópia (`ir_file`) e o caminho original só como informação.
- Testes: `cargo test` (54: DSP, afinador, detecção de corda e de harmônico, gate automático com o ruído real
  do cabo, filtro de zumbido, motor de áudio, presets, trocas sem estalo e o teclado/roda do knob simulados no egui
  sem janela). Medidas de custo, ignoradas por padrão: `cargo test --release -- --ignored --nocapture custo`.
- Afinações do usuário: `%APPDATA%\GuitarForge\afinacoes.json`.
- Ícone: palheta âmbar com knob de amp, gerado por `scripts/gerar_icone.py` (sem biblioteca de imagem) em
  `assets/` (`guitarforge.ico` de 16 a 256 px e `guitarforge.png`). O `build.rs` embute ícone e descrição no
  .exe com o `rc.exe` do Windows SDK; o PNG vira o ícone da janela. Mudou o ícone: rodar o script e recompilar.

## Cadeia de sinal (como está)

```
Guitarra > cabo Rocksmith (WASAPI exclusivo: 1 canal, 48 kHz, 16 bits, blocos de 3 ms; no compartilhado o cabo
            aparece com 2 canais iguais em float e blocos de 10 ms, e usa o 1º)
  > Nível de entrada (-12 a +24 dB)
  > Filtro de zumbido (entalhes de 2,5 Hz em 60, 120, ..., 360 Hz, em f64; 50 Hz se o cabo mostrar rede de 50)
  > Gate por energia (RMS em 10 ms, não pico: estalos do jack não o seguram aberto), histerese de 6 dB, espera de
    50 ms; fechamento (10 a 500 ms, padrão 80) e redução (10 a 90 dB, 90 = mudo) ajustáveis. Automático: 10 dB
    acima do menor nível dos últimos 5 s, teto de -40 dB, sobe no máximo 3 dB/s (nada muda de repente numa nota)
  > Compressor (sustain, volume, makeup automático)
  > Auto-wah (envelope em dB abre um passa-faixa SVF/TPT de 350 Hz até 1 a 3,5 kHz; sensibilidade, ressonância)
  > Oitava (uma nota por vez: fundamental seguida com histerese vira quadrada; abaixo = flip-flop vezes o volume,
           arredondado; acima = sinal vezes a quadrada da nota; direto)
  > Boost (0 a +20 dB, brilho em prateleira de 1,2 kHz até +9 dB)
  > Drive (overdrive tanh, distorção cúbica, fuzz assimétrico; ganho, tom, volume), sobreamostrado
  > Chorus (LFO senoidal, 7 a 12 ms)
  > Phaser (6 passa-tudo varridos de 200 Hz até 3,2 kHz, realimentação)
  > Flanger (0,3 a 4,3 ms varridos, realimentação até 0,85)
  > Tremolo (1 a 12 Hz; forma da senoide ao quase quadrado por tanh, sem degrau)
  > Amp, sobreamostrado (canais Limpo, Crunch e Lead; pré com 1 ou 2 estágios; tonestack depois do pré, neutro no meio;
         saturação de power amp)
  > Gabinete (3 voicings internos por filtros: Combo 1x12, Aberto 2x12, Fechado 4x12; ou IR em WAV por
              convolução particionada FFT, 64 amostras de latência, até 8192 pontos, normalizado em energia)
  > EQ de 5 faixas (100, 250, 800, 2,5k, 6k; mais ou menos 12 dB) e cortes de grave (20 a 400 Hz) e de agudo
       (2 a 20 kHz), Butterworth de 12 dB/oitava
  > Delay (40 a 1200 ms, repetições que escurecem, tempo desliza sem estalo)
  ===== daqui em diante, estéreo =====
  > Estéreo (dobra: cópia de 15 ms balançando 1,5 ms a 0,23 Hz, somada num lado e subtraída no outro, passa-altas de
            130 Hz na cópia; a soma dos lados é o som original, então mono não muda; largura de 0 a 1)
  > Reverb (Freeverb estéreo: 8 combs e 4 all-pass por lado, o direito 23 amostras mais longo)
  > Volume e limitador suave (linear até 0,7, encosta em 1,0), nos dois lados
  > Saída padrão do Windows, compartilhada ou exclusiva (esquerda e direita; o resto mudo em 5.1/7.1; saída mono
    recebe a média)
Em paralelo: afinador (YIN sobre o sinal sem zumbido), nível, ruído de fundo (mínimo de 5 s) e zumbido da rede
(Goertzel em 50 e 60 Hz, 1 s, sobre o sinal cru).
```

**Trocas sem estalo (01/11, `src/dsp/mod.rs`)**: ligar o drive no meio de uma nota dava um salto entre duas amostras
4,4 vezes maior que o do som parado (teste `switching_in_the_middle_of_a_note_does_not_click`). Agora:
- Ligar e desligar qualquer estágio (os 16, do filtro de zumbido ao reverb) é uma transição de 10 ms entre o som de
  antes dele e o de depois. Na transição o estágio roda e mistura com uma cópia do bloco (memória alocada uma vez,
  blocos de até 2048). Estágio religado do zero limpa a memória dele antes (o resto do som antigo não volta).
- O que não dá para misturar (canal do amp, tipo de drive, gabinete ou IR ligado) e a troca de preset ou de IR passam
  por um silêncio curto: o som abaixa em 5 ms **antes do delay e do reverb**, a troca entra no silêncio e volta em
  5 ms. O rabo do eco e da sala continua soando. A interface diz que é troca de preset (`send_settings(s, true)`).
- Com isso, as nove trocas do teste (pedais, canal, preset) saltam no máximo 1,0 vez o normal.

**Denormais (01/11)**: quando o gate fecha, os filtros, o gate e o reverb decaem até números abaixo de 1e-38, que o
processador calcula devagar: o silêncio depois de uma nota custava 40 a 55% a mais que tocar (teste ignorado
`custo_do_silencio_depois_da_nota`; com `FTZ=1` liga a proteção nele). A callback de entrada liga o FTZ e o DAZ do SSE
(`dsp::flush_denormals`) a cada bloco e o silêncio passa a custar o mesmo que tocar. Nesta máquina a folga já era
grande (1,7% do bloco); em processador fraco é a diferença entre picotar ou não no fim da nota.

## Arquitetura de tempo real

Nenhuma trava (mutex) no caminho do áudio. Cada parte conversa por anel lock-free (`rtrb`), canal (`crossbeam`) ou
atômico.

| Thread | Faz | Fala com |
|---|---|---|
| entrada (callback do cpal ou thread `áudio-entrada` do WASAPI exclusivo) | aplica ajustes novos, troca IR, processa a cadeia inteira | anel de saída, anel de análise, anel do gravador, medidores atômicos |
| saída (callback do cpal ou thread `áudio-saída`) | lê o anel com interpolação linear; reamostra se as taxas diferem; ajusta a razão pelo nível do anel para compensar a diferença de relógio entre o cabo e a placa | medidores (pico, falhas) |
| análise | YIN a cada 50 ms (janela de 4096 dizimada por 2; 8192 para baixo), RMS, piso de ruído, rede 50/60 Hz a cada 500 ms | medidores atômicos |
| gravador | escreve os dois WAV float 32 bits | anel do gravador |
| interface (egui) | edita `Settings` (Copy, sem alocação) e manda pelo canal quando muda; lê medidores; carrega IR e monta o convolver fora do áudio | canais e atômicos |

Folga do anel (Áudio… > Latência): Baixa = meio bloco, Normal = 1 bloco, Segura = 2 blocos de entrada.

**Abertura e lista de dispositivos (01/11)**: o `input_devices` e o `output_devices` do cpal testam formato por formato
de cada dispositivo, e o driver do JBL Quantum910 (saída Game) leva **5 s** para responder. Com ele escolhido como
saída o som levava 7 s para começar (log de 31/10) e o diálogo Áudio… congelava a tela 5 s a cada abertura. A lista
agora sai do `devices()` com o formato padrão de cada um (`devices_of`, 0,2 s no total) e o áudio abre em 80 a 140 ms.
A janela aparece antes: o motor só abre no 3º quadro, com "Abrindo o áudio…" no lugar do estado (antes a janela do
eframe ficava invisível até o áudio abrir, e quem esperava clicava no atalho de novo).
- **Uma janela só** (`src/instance.rs`): um mutex com nome; a segunda cópia sinaliza um evento com nome, dá à primeira
  o direito de vir para a frente (`AllowSetForegroundWindow`) e sai antes de criar o log (senão apagaria o da
  primeira, que foi o que aconteceu no primeiro teste). A primeira espera o evento numa thread e pede foco ao egui.
- **Cabo que some e volta**: com o erro de dispositivo, a interface olha a lista a cada 2 s e religa sozinha quando
  a entrada some e reaparece (travada e ainda listada, fica no botão "Tentar de novo"). No automático, se o app abriu
  sem o cabo (no microfone padrão), troca para o cabo quando ele aparece.
- **Buracos na entrada exclusiva**: pacote marcado pelo Windows como descontínuo (a thread atrasou e perdeu áudio do
  cabo) conta em `in_glitches`, mostrado com as falhas no rodapé, no `--teste-motor` e no `--diagnostico`.

### Modo exclusivo (31/10, `src/audio/wasapi_io.rs`)

O cpal 0.15 só abre o WASAPI compartilhado, com o período fixo de 10 ms do mixer do Windows. O modo exclusivo usa o
crate `wasapi` direto, **por lado** (Áudio… > Modo exclusivo: Entrada, Saída), com queda automática para o
compartilhado quando o Windows nega (o motivo aparece no diálogo). Cada lado é uma thread com prioridade "Pro Audio"
do MMCSS; a cadeia, o anel e a correção de relógio são os mesmos do compartilhado. Padrão: entrada exclusiva ligada
(o cabo só serve ao GuitarForge), saída desligada (exclusiva, ela cala o resto do Windows naquele dispositivo).

Medido nesta máquina com `--wasapi` (todos os drivers):
- Cabo Rocksmith: exclusivo em 1 canal, 48 ou 44,1 kHz, 16 bits; período mínimo 3 ms.
- JBL Quantum910 (Game): exclusivo em 2 canais, 48 kHz, 16 bits; mínimo 3 ms. Realtek: até 24 bits.
- `IAudioClient3` (compartilhado de período curto): nenhum driver daqui desce de 10 ms, então não vale a pena.

**Defeito do JBL Quantum910 no exclusivo (31/10, causa do "som incompreensível" ouvido no teste)**: o dongle,
aberto no exclusivo por evento, às vezes pede 18 mil amostras/s com período de 3 ms (32 mil com 10 ms) em vez de 48 mil,
e toca buracos e repetições; em outras aberturas pede os 48 mil certos (2 de 4 numa rodada). Cada "Aplicar" do diálogo
reabre o fone, por isso parecia que a sobreamostragem ou a latência estragavam o som (offline a cadeia a 4x dá
correlação 0,9996 com a de 1x). Escrever pelo espaço livre (`GetCurrentPadding`) dá a mesma taxa: é o dispositivo.
Solução: a thread da saída mede o ritmo dos pedidos a partir do 50º; erro grosseiro (mais de 15%) aparece em 0,25 s,
e o fino (3%) em 1 s. Como o defeito é sorteado a cada abertura, ela **reabre o fone no exclusivo até duas vezes**
(01/11: em cinco aberturas, uma pegou o defeito duas vezes seguidas e acertou na terceira, 16 ms; antes ela caía no
compartilhado, 42 ms). Se as três errarem, troca para o compartilhado no mesmo dispositivo (`meters.out_fallback`),
sem parar a entrada; a interface só mostra o aviso no diálogo. A conferência fica na thread porque o eframe para de
desenhar com a janela coberta.

**O que mais o JBL Quantum910 tem (01/11, ficha da JBL e medido aqui)**:
- O dongle cria **duas placas**: Game (8 canais, com o efeito de software "JBL 3D effect & Equalizer" do
  QuantumENGINE 2.0.5 registrado no Windows) e Chat (2 canais, sem efeito nenhum). O exclusivo pula o efeito do Game;
  no compartilhado ele colore o som do amp com o EQ e o 3D do JBL.
- A **Chat no exclusivo** funciona: 15,0 ms, 0 falhas, 48006 amostras/s (`--teste-motor 6 ambos normal "Fone de
  ouvido do headset (JBL Quantum910 Wireless Chat)"`). O botão de equilíbrio Game/Chat do fone vira um mixer de
  guitarra e música: GuitarForge na Chat, YouTube e afins na Game (a padrão do Windows). A conferir: se o Windows
  abaixa os outros sons quando algo abre a Chat (Som > Comunicações).
- Cabo de 3,5 mm: o fone funciona **passivo** (sem bateria, 20 Hz a 40 kHz), sem o atraso do rádio, numa saída
  analógica do PC.
- Bluetooth 5.2 ao mesmo tempo que o dongle: música do celular pelo Bluetooth e a guitarra pelo dongle, sem mixar no PC.
- ANC e "talk-through" só pelos botões do fone; o head tracking (QuantumSPHERE 360) passa pelo QuantumENGINE na Game.

**Anel com saída compartilhada (31/10)**: com a entrada exclusiva (blocos de 3 ms) e a saída do mixer (10 ms), a folga
em blocos de entrada ficou curta e o teto do anel não contava o bloco de saída: 4 falhas e 2270 descartes em 15 s, o
padrão de fábrica estalava. Agora o teto é o alvo mais duas vezes o maior bloco (entrada ou saída), e a folga tem uma
reserva pelo tremor do mixer (2 ms na saída compartilhada, 0 na exclusiva) que cresce um bloco a cada falha, até
20 ms. Medido no regime, 15 s cada: compartilhado, entrada exclusiva e as duas exclusivas, em Normal e Baixa, todos
com 0 falhas e 0 descartes (a não ser o JBL abrindo com defeito, que agora cai para o compartilhado).

Decisões tiradas de teste no aparelho:
- **Saída por evento, não por sondagem.** A crate avisa que o evento pode picotar no driver USB padrão; mas na
  sondagem o dongle sem fio do JBL não consome nada (a fila fica parada para sempre), e por evento funciona nele e na
  Realtek. O rádio do JBL leva ~1 s para acordar, por isso a saída só se dá por perdida depois de 2 s sem pedido.
- Entrada por evento, um pacote por evento, lido só se `GetCurrentPadding` > 0 (ler pacote vazio faria um
  `ReleaseBuffer` fora de ordem).
- Exclusivo **toma** o dispositivo de quem o usa no compartilhado (padrão do Windows "dar prioridade a aplicativos no
  modo exclusivo"): o outro programa perde o áudio até reabrir.

Atraso do app medido com `--teste-motor 10` (cabo > JBL, motor mudo; o compartilhado ainda soma o mixer do Windows,
uns 10 a 20 ms, e o rádio do fone sem fio soma o dele nos dois casos):

| Modo | Mediana | Falhas |
|---|---|---|
| compartilhado (até a v0.2) | 36,6 ms | 0 |
| só entrada exclusiva (padrão) | 27 ms | 0 depois do conserto do anel |
| entrada e saída exclusivas | 15,0 ms (quando o JBL abre certo) | 0 |
| exclusivas, latência Baixa | 13,6 ms | 0 |

Latência Baixa e Normal pedem período de 3 ms; Segura pede 10 ms. A folga do anel continua em blocos de entrada.

## Interface (v0.2)

- Topo: status do cabo (LED verde = Rocksmith achado, amarelo = outra entrada, vermelho = sem áudio, com "Tentar de
  novo"), GRAVAR, medidores de entrada (com CLIP) e saída, knob de volume, botão MUDO.
- Linha de preset: fábrica (Limpo cristalino, Blues, Crunch clássico, Rock, Solo com delay, Metal, Ambiente, Direto) e
  "Meus presets"; "(alterado)" e Desfazer; trocar de preset preserva volume, mudo e nível de entrada.
- Cadeia clicável (Cabo > Gate > ... > Saída): cada etapa liga e desliga.
- Afinador sempre visível e **por corda**, como no Rocksmith (detalhes e o caso real que motivou cada regra na
  seção "Afinador" abaixo). 26 afinações de fábrica (guitarra de 6, 7 e 8 cordas; baixo de 4, 5 e 6) e afinações do
  usuário (nome, instrumento, 4 a 9 cordas, nota de cada corda, observação para artista ou música). Notação
  Dó Ré Mi (padrão) ou C D E. Preset salvo guarda a afinação e carregá-lo troca o afinador.
- Layout: coluna central de no máximo 1360 px; cada fileira de cartões reparte a largura toda (proporcional às
  larguras mínimas), então as bordas alinham em qualquer tamanho e maximizado fica centralizado.
- Pedais em fileiras equilibradas (`balanced_rows`): o menor número de fileiras que cabe na largura, com o mesmo
  número de cartões em cada uma e a ordem do sinal (1400 px: 5 e 5; 1000 px: 3, 3, 2 e 2). A cadeia quebra linha.
- Pedais como cartões com chave de pisar e LED verde. Knobs: arrastar para cima ou para a direita (Shift = fino),
  duplo clique volta ao padrão; **clique seleciona** (anel colorido) e aí setas, Shift + setas (fino),
  PageUp/PageDown, Home/End, Delete (padrão) e a roda do mouse ajustam, Esc solta, Tab vai ao próximo. A roda só
  mexe no knob selecionado (passando por cima, ela rola a página). EQ com faders próprios no mesmo esquema.
- Cartão Entrada e Gate: "Automático" (limiar mostrado no knob apagado), "Tirar zumbido", Espera (0 a 2 s) e Fecha
  (10 ms a 2 s) para nota longa não ser cortada, Redução. A supressão de ruído (gate e zumbido) é do cabo e da
  guitarra, não do som: trocar de preset não mexe nela (como o nível de entrada), e o botão "Supressão de ruído" na
  linha do preset (tecla N) liga e desliga as duas coisas juntas. O WAV (DI) da gravação sai sempre sem corte.
- Rodapé de diagnóstico: ruído de fundo, zumbido da rede (baixo, moderado, alto), estado do gate, falhas de áudio.
- Atalhos: M mudo, T afinador, R gravar, N supressão de ruído.
- Cores todas em `src/gui/theme.rs` (base para a fase personalizável).

## Afinador: regras e por quê (testado com o cabo Rocksmith e uma Stratocaster)

- **Automático por padrão** (descobre a corda). Clicando numa corda, trava nela e, quando ela fica afinada (5 cents
  por 0,7 s), passa para a próxima. Travado, se o som bater com a nota de outra corda, aparece uma dica amarela
  ("esse é o som da 4ª corda; se foi a 6ª, ela está esticada demais") em vez do alarme. Motivo: com o guiado como
  padrão, toda corda tocada aparecia como "muito acima da 6ª" e não dava para afinar nada (30/10).
- **Automático não troca de corda enquanto a nota desliza** (tarraxa girando, menos de 80 cents entre leituras):
  na gravação de 30/10 a 6ª, girada para o lado errado, passou a ficar mais perto do Lá, o afinador disse "5ª corda,
  aperte" e ela foi esticada até 113 Hz (Mi é 82). Teste `turning_the_peg_never_jumps_to_the_neighbor_string`.
- **Corda escolhida = busca dirigida** de 9 semitons abaixo a 7 acima do alvo, confrontada com a busca livre. Se uma é
  harmônica da outra (x2 a x6), decide a energia na mais grave: existe fundamental (fraca, mas existe) = é ela; não
  existe = era só múltiplo do período e vale a mais aguda (outra corda). Caso real: 6ª a 80 Hz lida como 320 Hz
  (4º harmônico) com "afrouxe já".
- **YIN com limiar relativo** ao vale mais fundo (ideia do MPM de McLeod) em vez de fixo.
- **Aviso de corda esticada** acima de 200 cents do alvo.
- Diagnóstico de gravações com as ferramentas `--analisar` e `--espectro`.

## Próximas fases

### Personalizável (depois da v0.2)
- [ ] Reordenar, adicionar e remover pedais (arrastar e soltar), mais de um do mesmo tipo
- [ ] Temas e cores (claro, escuro, alto contraste) e escala da interface
- [ ] Esconder ou mostrar seções; afinador compacto ou tela cheia
- [x] Afinações alternativas, notação Dó Ré Mi / C D E, baixo e 7/8 cordas, afinações do usuário (feito na v0.2)
- [ ] Referência diferente de A = 440 Hz (432, 442...)
- [ ] Atalhos e pedal MIDI configuráveis

### Som
- [x] Backend WASAPI exclusivo (31/10; o `IAudioClient3` foi medido e não ajuda nos drivers desta máquina)
- [ ] NAM (carregar .nam da comunidade, inferência própria)
- [x] Sobreamostragem do drive e do amp (31/10): Áudio… > Sobreamostragem, Desligada, 2x ou 4x (padrão). Degraus de 2x
  com FIR meia-banda de 47 coeficientes (Kaiser, ~70 dB), 0,48 ms por degrau; o 7º harmônico de 5 kHz que voltava
  em 13 kHz cai mais de 30 dB a 4x (teste). Custo com todos os pedais: 1,0% de um núcleo sem, 1,6% a 2x, 2,7% a 4x
  (`cargo test --release -- --ignored --nocapture custo`).
- [x] Auto-wah, oitava, boost, phaser, flanger, tremolo; gate com fechamento e redução; cortes no EQ (31/10)
- [ ] Wah por pedal de expressão (MIDI) e posição fixa
- [x] Trocas sem estalo: pedais com transição de 10 ms, canal, drive, gabinete, IR e preset por um silêncio curto (01/11)
- [x] IR guardado com o preset (cópia na pasta do app) (01/11)
- [x] Saída estéreo: Estéreo (dobra, mono compatível) e reverb estéreo; gravação processada em estéreo (01/11)
- [ ] IR longa (sala, reverb por convolução) em partições de tamanhos diferentes
- [ ] Nivelar o volume entre presets e canais
- [ ] Looper, metrônomo, backing track
- [ ] Do fone (barato ao caro): aviso de que fone sem fio soma atraso (e a dica do cabo de 3,5 mm e da Chat como
  mixer); medir o atraso real até o ouvido com a concha do fone encostada no captador (o single coil pega o campo do
  alto-falante); amp "na sala" pelos canais 7.1 da Game e o head tracking do JBL (só no compartilhado, +20 ms).

## Diferenciais vs concorrência

Conferido com os dois apps instalados (31/10).

| Recurso | Blue Cat Free Amp | TONE3000 | NAM Plugin | GuitarForge |
|---|---|---|---|---|
| Standalone sem DAW | sim | sim | não | sim |
| Acha o cabo Rocksmith sozinho | não | não | não | sim |
| WASAPI exclusivo | ? | sim (e ASIO) | não | sim |
| NAM | não | sim (só A2) | sim | futuro |
| IR loader | sim | sim | não | sim |
| Blocos em cadeia | não | sim (NAM e IR) | não | 10 pedais fixos |
| Efeitos (wah, oitava, modulação, delay) | só reverb | pitch shift | não | sim |
| Afinador integrado | não | sim (cromático) | não | sim, por corda |
| Noise gate | não | sim | não | sim, automático |
| Medidor de zumbido e ruído | não | não | não | sim |
| Gravador (DI + processado) | não | não | não | sim |
| Open-source | não | sim (MIT) | sim (MIT) | sim (MIT) |
