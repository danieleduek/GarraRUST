- **Roteador do admin so despacha para pagina declarada (#2).** O nome da
  pagina vem do `#hash` da URL e era conferido com `!pages[page]` num objeto,
  que aceita qualquer propriedade herdada de `Object.prototype`
  (`#constructor`, `#toString`) e a chamava como se fosse pagina. A tabela de
  paginas virou um `Map`, que so conhece o que foi declarado (CodeQL
  js/unvalidated-dynamic-method-call).
