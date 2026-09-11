- **`garra chat` para de descartar turno saudavel no meio do stream (#1).**
  `--timeout-secs` era prazo de duracao total do turno, entao um turno lento
  porem vivo — loop agentico com varias voltas de ferramenta, ou o modelo local
  default, um 27B de ~18 GB — era jogado fora aos 120s, e o texto ja impresso
  sumia do historico. O prazo passou a ser de inatividade: rearmado a cada
  evento do stream, so fecha quando o provedor emudece por uma janela inteira,
  que era o caso que ele sempre quis pegar. O card de erro passou a falar em
  silencio em vez de duracao e aponta o `garra doctor`.
