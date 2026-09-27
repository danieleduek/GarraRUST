- **Roteador do admin so despacha para pagina declarada (#2).** O nome da
  pagina vem do `#hash` da URL e era conferido com `!pages[page]`, que aceita
  qualquer propriedade herdada de `Object.prototype` (`#constructor`,
  `#toString`) e a chamava como se fosse pagina. A checagem passa a ser
  `hasOwnProperty` (CodeQL js/unvalidated-dynamic-method-call).
