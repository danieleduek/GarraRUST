- **Gateway para de cortar aos 120 s uma resposta de LLM que so era longa (#2).**
  O bootstrap montava o cliente HTTP dos providers com prazo de duracao TOTAL
  (`timeouts.llm.default_secs`), corpo em streaming incluso, entao um turno
  vivo porem lento — loop agentico de varias voltas, ou o modelo local
  default, um 27B de ~18 GB — era cortado no meio e chegava ao runtime como
  "stream read error", sem dizer que era prazo. O prazo passou a ser de
  inatividade (sem bytes chegando), como o `garra chat` ja fazia; zero
  desliga. O mesmo cliente passa a nao seguir redirects, como o cliente
  default de cada provider ja fazia (#1248, regra 14).
