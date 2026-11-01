# Novidades

## 0.5.0 (01/11/2026)

- Reverb com sala por convolução: cinco salas de fábrica, geradas pelo próprio app (Quarto, Estúdio, Sala, Salão e
  Igreja, de 0,4 a 3,8 segundos), ou o IR de uma sala real em WAV, mono ou estéreo, de até 8 segundos, guardado com o
  preset como o IR de gabinete. Pré-atraso de até 150 ms. A sala acrescenta só 1,3 ms de atraso e gasta menos de 1% do
  processador. Trocar de sala, ou voltar ao reverb de algoritmo, faz uma transição curta, sem cortar o rabo.
- Volume igual entre presets: ao carregar um preset, o app mede o volume dele com um acorde de referência (do jeito que
  o ouvido percebe, pela norma do LUFS) e compensa. Antes, com a mesma força na corda, os presets de fábrica variavam
  até 26 dB e o Direto saía uns 13 dB abaixo do Limpo cristalino. Vale também para os presets do usuário; o botão
  "Volume igual", ao lado da supressão de ruído, desliga.
- Os canais Limpo, Crunch e Lead do amp saem no mesmo volume com os knobs no padrão (antes o Lead saía 12 dB acima
  do Limpo).
- Avisos da saída exclusiva. Com ela, o som do GuitarForge passa por fora do mixer do Windows: nenhum outro programa
  consegue capturar (o OBS, o Discord, compartilhar a tela com som no navegador) e quem já tocava nessa saída pode
  ficar mudo até ser reaberto. O diálogo Áudio… avisa ao marcar a saída exclusiva, com mais força quando ela é a saída
  padrão do Windows; o topo mostra "saída exclusiva" com um link para liberar; e uma faixa avisa, com o botão
  "Liberar a saída", quando OBS, Streamlabs, XSplit, Discord, Teams, Zoom ou Audacity estão abertos. Liberar passa a
  saída para o modo compartilhado, que os outros programas capturam, com uns 10 ms a mais de atraso.
- Correção: a bolha de status da barra de cima ficava uns 9 pixels abaixo dos botões ao lado.

## 0.4.0 (01/11/2026)

- O som abre em menos de um segundo. Com alguns fones sem fio escolhidos como saída (medido num JBL Quantum910), cada
  consulta à lista de dispositivos levava 5 segundos: o som demorava uns 7 segundos para começar e o diálogo de áudio
  congelava a tela.
- A janela aparece na hora, com "Abrindo o áudio…", e o app abre uma janela só: clicar de novo no atalho traz para a
  frente a que já está aberta.
- Trocas sem estalo. Ligar e desligar um pedal no meio da nota faz uma transição curta; trocar o canal do amp, o tipo
  de drive, o gabinete, o IR ou o preset passa por um silêncio de poucos milissegundos antes do delay e do reverb, que
  continuam soando.
- Saída estéreo: cartão novo Estéreo, que abre o som no fone e deixa o grave no meio (numa caixa mono soa igual), e
  reverb estéreo. A gravação processada sai em estéreo; o DI continua mono.
- O IR de gabinete fica guardado com o preset, numa cópia na pasta do app: mover ou apagar o arquivo original não
  quebra mais o som. Se um IR não existir mais, o app volta ao gabinete interno e avisa.
- Saída exclusiva que abre na taxa errada (defeito visto num fone sem fio USB) é reaberta até duas vezes antes de
  passar para o modo compartilhado.
- O app religa sozinho quando o cabo volta e, no automático, passa para o cabo quando ele é ligado com o app aberto.
- Correções: o silêncio depois da nota gastava mais processador que tocar; o fuzz deixava um deslocamento no sinal; os
  atalhos M, T, R e N não funcionavam com um knob selecionado; em janela estreita o botão Gravar cobria o botão
  Áudio…; o estado e os presets agora são gravados sem risco de ficar pela metade.

## 0.3.0 (31/10/2026)

- Modo exclusivo do WASAPI, escolhido por lado em Áudio…: com entrada e saída exclusivas o atraso do app cai de uns
  37 ms para uns 15 ms. A entrada exclusiva vem ligada (o cabo só serve ao GuitarForge); a saída é opcional, porque
  enquanto o app estiver aberto os outros programas ficam sem som nela.
- Proteção para fone que não aguenta o exclusivo: o app confere o ritmo que o dispositivo pede logo depois de abrir e,
  se estiver errado, passa sozinho para o modo compartilhado e avisa no diálogo de áudio. Medido num fone sem fio USB
  que às vezes abre pedindo 18 mil amostras por segundo em vez de 48 mil.
- Seis pedais novos: auto-wah, oitava (abaixo e acima), boost, phaser, flanger e tremolo.
- Sobreamostragem do drive e do amp (2x ou 4x, padrão 4x): some o chiado metálico da distorção forte.
- Supressão de ruído com botão próprio (tecla N), que vale para todos os presets. O gate ganhou Espera e Fecha de até
  2 segundos, para nota longa não ser cortada, e Redução, para abaixar em vez de cortar.
- Equalizador com corte de graves e de agudos.
- Quatro presets de fábrica novos: Funk com wah, Surf com tremolo, Psicodélico e Rock com boost.
- Os pedais se arrumam em fileiras conforme a largura da janela.
- Correção: com a entrada exclusiva e a saída compartilhada o som estalava (a folga entre entrada e saída era curta).
- Ferramentas novas de diagnóstico: `--wasapi`, `--teste-motor`, `--diagnostico` e `--print`.

## 0.2.0 (30/10/2026)

- Primeira versão pública: afinador por corda, gate automático com filtro de zumbido, compressor, drive, chorus, amp
  de três canais, gabinete interno ou IR, equalizador, delay, reverb, gravador e presets.
