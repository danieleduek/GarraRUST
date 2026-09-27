- **Auto-reset do teto por turno deixa de apagar a memoria do detector de
  loop (#2).** A cada 10 chamadas de ferramenta o runtime renovava o teto por
  turno com `resetar_turno`, que tambem esvaziava a janela de tres chamadas
  identicas — o detector esquecia o que tinha visto bem no meio de um loop.
  O auto-reset agora so zera o contador, e a linha de log passa a dizer os
  numeros (`orcamento=turn=10/10 task=n/50`).
