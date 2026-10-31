# Novidades

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
