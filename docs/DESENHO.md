# GuitarForge: estúdio virtual de guitarra

Amp digital completo, offline, sem conta, open-source (MIT). Cabo Rocksmith USB direto no PC.
Um único executável: liga o cabo, abre o app, escolhe o som, toca.

## Estado (30/10/2026, v0.2)

Interface amigável pronta e testada com o cabo real. A fase "personalizável" fica para depois (lista no fim).
- Rodar: `cargo run --release`; executável em `target\release\guitarforge.exe`.
- Ferramentas de diagnóstico (build debug, que tem console):
  - `--dispositivos`: entradas e saídas com o formato de cada uma.
  - `--analisar arquivo.wav [fmin] [Hz da corda]`: roda o afinador sobre uma gravação (a cada 50 ms).
  - `--espectro arquivo.wav inicio fim`: picos do espectro de 40 a 700 Hz (diz qual nota soava de fato).
  - `--ruido arquivo.wav inicio fim`: pico e RMS cru e sem zumbido (o que o gate enxerga).
- Estado, presets do usuário e log: `%APPDATA%\GuitarForge\` (`estado.json`, `presets\*.json`, `guitarforge.log`).
- Gravações: `%USERPROFILE%\Music\GuitarForge\GuitarForge NNN.wav` (processado) e `... (DI).wav` (seco do cabo).
- Testes: `cargo test` (33: DSP, afinador, detecção de corda e de harmônico, gate automático com o ruído real
  do cabo, filtro de zumbido, motor de áudio, presets e o teclado/roda do knob simulados no egui sem janela).
- Afinações do usuário: `%APPDATA%\GuitarForge\afinacoes.json`.
- Ícone: palheta âmbar com knob de amp, gerado por `scripts/gerar_icone.py` (sem biblioteca de imagem) em
  `assets/` (`guitarforge.ico` de 16 a 256 px e `guitarforge.png`). O `build.rs` embute ícone e descrição no
  .exe com o `rc.exe` do Windows SDK; o PNG vira o ícone da janela. Mudou o ícone: rodar o script e recompilar.

## Cadeia de sinal (como está)

```
Guitarra > cabo Rocksmith (WASAPI compartilhado, 48 kHz, o cabo aparece com 2 canais iguais; usa o 1º)
  > Nível de entrada (-12 a +24 dB)
  > Filtro de zumbido (entalhes de 2,5 Hz em 60, 120, ..., 360 Hz, em f64; 50 Hz se o cabo mostrar rede de 50)
  > Gate por energia (RMS em 10 ms, não pico: estalos do jack não o seguram aberto), histerese de 6 dB, espera de
    50 ms, fade de 80 ms. Automático: 10 dB acima do menor nível dos últimos 5 s, teto de -40 dB, sobe no máximo
    3 dB/s (nada muda de repente durante uma nota)
  > Compressor (sustain, volume, makeup automático)
  > Drive (overdrive tanh, distorção cúbica, fuzz assimétrico; ganho, tom, volume)
  > Chorus (LFO senoidal, 7 a 12 ms)
  > Amp (canais Limpo, Crunch e Lead; pré com 1 ou 2 estágios; tonestack depois do pré, neutro no meio;
         saturação de power amp)
  > Gabinete (3 voicings internos por filtros: Combo 1x12, Aberto 2x12, Fechado 4x12; ou IR em WAV por
              convolução particionada FFT, 64 amostras de latência, até 8192 pontos, normalizado em energia)
  > EQ de 5 faixas (100, 250, 800, 2,5k, 6k; mais ou menos 12 dB)
  > Delay (40 a 1200 ms, repetições que escurecem, tempo desliza sem estalo)
  > Reverb (Freeverb: 8 combs e 4 all-pass)
  > Volume e limitador suave (linear até 0,7, encosta em 1,0)
  > Saída padrão do Windows (só frente esquerda e direita; o resto mudo em saídas 5.1/7.1)
Em paralelo: afinador (YIN sobre o sinal sem zumbido), nível, ruído de fundo (mínimo de 5 s) e zumbido da rede
(Goertzel em 50 e 60 Hz, 1 s, sobre o sinal cru).
```

## Arquitetura de tempo real

Nenhuma trava (mutex) no caminho do áudio. Cada parte conversa por anel lock-free (`rtrb`), canal (`crossbeam`) ou
atômico.

| Thread | Faz | Fala com |
|---|---|---|
| callback de entrada (cpal) | aplica ajustes novos, troca IR, processa a cadeia inteira | anel de saída, anel de análise, anel do gravador, medidores atômicos |
| callback de saída (cpal) | lê o anel com interpolação linear; reamostra se as taxas diferem; ajusta a razão pelo nível do anel para compensar a diferença de relógio entre o cabo e a placa | medidores (pico, falhas) |
| análise | YIN a cada 50 ms (janela de 4096 dizimada por 2; 8192 para baixo), RMS, piso de ruído, rede 50/60 Hz a cada 500 ms | medidores atômicos |
| gravador | escreve os dois WAV float 32 bits | anel do gravador |
| interface (egui) | edita `Settings` (Copy, sem alocação) e manda pelo canal quando muda; lê medidores; carrega IR e monta o convolver fora do áudio | canais e atômicos |

Folga do anel (Áudio… > Latência): Baixa = meio bloco, Normal = 1 bloco, Segura = 2 blocos de entrada.

**Latência medida (30/10)**: o app soma uns 40 ms (bloco de entrada de 10 ms + anel + bloco de saída de 10 ms, que é o
período fixo do WASAPI compartilhado); o Windows e o headset sem fio somam o deles por cima. Para tocar com conforto
o caminho é o modo exclusivo ou o `IAudioClient3` de baixa latência (blocos de 2,7 ms), que o cpal 0.15 não oferece:
próximo passo técnico é um backend próprio com o crate `wasapi`. O Rocksmith faz isso (`ExclusiveMode=1` no ini).

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
- Pedais como cartões com chave de pisar e LED verde. Knobs: arrastar para cima ou para a direita (Shift = fino),
  duplo clique volta ao padrão; **clique seleciona** (anel colorido) e aí setas, Shift + setas (fino),
  PageUp/PageDown, Home/End, Delete (padrão) e a roda do mouse ajustam, Esc solta, Tab vai ao próximo. A roda só
  mexe no knob selecionado (passando por cima, ela rola a página). EQ com faders próprios no mesmo esquema.
- Cartão Entrada e Gate: "Automático" (limiar mostrado no knob apagado) e "Tirar zumbido".
- Rodapé de diagnóstico: ruído de fundo, zumbido da rede (baixo, moderado, alto), estado do gate, falhas de áudio.
- Atalhos: M mudo, T afinador, R gravar.
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
- [ ] Backend WASAPI exclusivo / IAudioClient3 (latência baixa de verdade)
- [ ] NAM (carregar .nam da comunidade, inferência própria)
- [ ] Sobreamostragem 2x ou 4x no amp e no drive (tira o aliasing em ganho alto)
- [ ] Wah, octaver, tremolo, flanger, phaser, boost
- [ ] Looper, metrônomo, backing track

## Diferenciais vs concorrência

| Recurso | Blue Cat | TONE3000 | NAM Plugin | GuitarForge v0.2 |
|---|---|---|---|---|
| Standalone sem DAW | sim | sim | não | sim |
| Acha o cabo Rocksmith sozinho | não | não | não | sim |
| NAM | não | sim | sim | futuro |
| IR loader | sim | sim | não | sim |
| Pedais em cadeia | não | não | não | sim (fixos por enquanto) |
| Afinador integrado | não | não | não | sim |
| Medidor de zumbido e ruído | não | não | não | sim |
| Gravador (DI + processado) | não | não | não | sim |
| Noise gate | não | não | não | sim |
| Open-source | não | sim | sim (MIT) | sim (MIT) |
