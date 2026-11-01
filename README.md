# GuitarForge

Amp de guitarra para Windows feito para o cabo Rocksmith USB (qualquer entrada de áudio também serve).
Funciona offline, sem conta e é código aberto.

![Tela do GuitarForge](docs/tela.png)

## Baixar

Pegue o `GuitarForge-x.y.z-windows-x64.exe` da [versão mais recente](https://github.com/NBN-PATRIC/guitarforge/releases/latest)
e abra. Não precisa instalar nada. Como o executável não é assinado, o Windows pode mostrar "O Windows protegeu o
computador": clique em "Mais informações" e depois em "Executar assim mesmo". O que mudou em cada versão está em
[CHANGELOG.md](CHANGELOG.md).

## O que tem

- Acha o cabo Rocksmith sozinho e toca pela saída padrão do Windows, reamostrando se as taxas forem diferentes.
- Modo exclusivo do WASAPI por lado (entrada e saída), com volta automática ao compartilhado se o dispositivo falhar.
- Afinador por corda, automático ou guiado: 26 afinações de fábrica (guitarra de 6, 7 e 8 cordas, baixo de 4, 5 e 6)
  e afinações suas, com notação Dó Ré Mi ou C D E.
- Supressão de ruído com botão próprio: gate com limiar automático, que acompanha o ruído do seu cabo, espera e
  fechamento ajustáveis até 2 segundos, e filtro do zumbido da rede (50 ou 60 Hz, detectado sozinho).
- Pedais: compressor, auto-wah, oitava, boost, drive (overdrive, distorção e fuzz), chorus, phaser, flanger e tremolo.
- Sobreamostragem do drive e do amp (2x ou 4x) contra o chiado metálico da distorção forte.
- Amplificador com canais Limpo, Crunch e Lead; gabinete interno ou IR em WAV, guardado junto com o preset.
- Equalizador de 5 faixas com corte de graves e de agudos, delay, estéreo (abre o som no fone e deixa o grave no
  meio) e reverb estéreo.
- Pisar num pedal ou trocar de preset no meio da nota não estala.
- Gravador: salva o som processado (estéreo) e o sinal seco do cabo (DI) em WAV.
- Presets de fábrica e seus, que guardam também a afinação.
- Diagnóstico do ruído de fundo e do zumbido que chegam pelo cabo.
- Tudo ajustável pelo teclado: clique num knob e use as setas (Shift para ajuste fino).

## Como compilar

Precisa do Rust (rustup) e das ferramentas de build do Visual Studio com o Windows SDK, de onde vem o `rc.exe` que
embute o ícone no executável.

```
cargo run --release
```

O executável fica em `target\release\guitarforge.exe`. `cargo test` roda os testes de DSP, afinador, gate, motor de
áudio e interface.

## Atalhos

M liga e desliga o som, T entra no modo afinação, R grava, N liga e desliga a supressão de ruído.

## Diagnóstico

A build de desenvolvimento (`cargo run`) abre um console e traz algumas ferramentas:

- `--dispositivos`: lista as entradas e saídas de áudio com o formato de cada uma.
- `--analisar arquivo.wav [fmin] [Hz da corda]`: roda o afinador sobre uma gravação.
- `--espectro arquivo.wav inicio fim`: picos do espectro de 40 a 700 Hz no trecho.
- `--ruido arquivo.wav inicio fim`: ruído do trecho, cru e sem o zumbido da rede.
- `--wasapi`: o que cada dispositivo aceita no modo exclusivo.
- `--teste-motor [segundos] [entrada|saida|ambos|nenhum]`: liga o áudio mudo e mede atraso, falhas e descartes.
- `--diagnostico segundos [entrada|saida|ambos|nenhum] [baixa|normal|segura] [1|2|4] pasta`: grava o cabo, a saída
  da cadeia e o que foi mandado ao dispositivo enquanto você toca, e compara os três.

## Limitações

O atraso do próprio app fica perto de 15 ms com entrada e saída no modo exclusivo e de 27 a 40 ms com a saída no
modo compartilhado. Fone sem fio soma o atraso do rádio dele por cima; com fio é o melhor para tocar. Arquitetura,
decisões e próximas fases em [docs/DESENHO.md](docs/DESENHO.md).

## Aviso

Projeto independente, sem relação com a Ubisoft. Rocksmith é marca da Ubisoft. O GuitarForge não usa código, áudio
nem dados do jogo: só lê o cabo, que o Windows reconhece como um microfone USB comum.

## Licença

MIT. Veja [LICENSE](LICENSE).

## English

GuitarForge is a guitar amp for Windows built around the Rocksmith USB cable (any audio input works too). It runs
offline, needs no account and is open source. It has a per string tuner (guitar and bass, custom tunings), a noise
gate with automatic threshold and mains hum filter, WASAPI exclusive mode (about 15 ms of app latency), compressor,
auto wah, octave, boost, drive, chorus, phaser, flanger, tremolo, a three channel amp with 2x or 4x oversampling,
built in cabinets or WAV impulse responses, EQ with low and high cut, delay, a stereo widener, stereo reverb and a
recorder that saves both the processed (stereo) and the dry signal. The interface is in Portuguese. Download the executable from the Releases page, or build
with `cargo run --release` (Rust plus the Visual Studio Build Tools with the Windows SDK). Not affiliated with
Ubisoft. MIT license.
