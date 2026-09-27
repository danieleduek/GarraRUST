use std::pin::Pin;

use async_trait::async_trait;
use futures::Stream;
use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};

/// #1298: resultado da validação de um identificador de modelo contra o
/// catálogo real do provider — o insumo da decisão transacional do `/model`
/// no CLI: ou valida e troca, ou não altera o estado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidacaoDeModelo {
    /// O catálogo completo do provider contém o modelo.
    Listado,
    /// O catálogo completo contém, mas a lista curada (`/models`) não
    /// anuncia — rota válida com nome não anunciado (ex.: namespace de
    /// terceiro servido pelo OpenRouter, como `z-ai/...`).
    ListadoForaDaCurada,
    /// O catálogo foi obtido e NÃO contém o modelo — a troca deve ser
    /// recusada com o estado anterior intacto.
    Ausente,
    /// O provider não expõe catálogo — a validação é impossível por design;
    /// quem decide a política é o chamador.
    SemListagem,
}

/// Trait para integrações com provedores de LLM (Anthropic, OpenAI, Ollama, etc.).
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Identificador do provedor (ex: "anthropic", "openai", "ollama").
    fn provider_id(&self) -> &str;

    /// Envia uma requisição de completion e retorna a resposta.
    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse>;

    /// Envia uma requisição de completion em modo streaming,
    /// retornando eventos conforme são recebidos.
    /// A implementação padrão retorna erro indicando que streaming não é suportado.
    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        Err(garraia_common::Error::Agent(format!(
            "provedor {} não suporta streaming",
            self.provider_id()
        )))
    }

    /// Retorna o modelo padrão configurado para o provedor, se conhecido.
    fn configured_model(&self) -> Option<&str> {
        None
    }

    /// Retorna a lista de modelos disponíveis para este provedor.
    async fn available_models(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// #1298: valida um identificador de modelo contra o catálogo REAL do
    /// provider — não a lista curada que `available_models` devolve.
    ///
    /// O padrão reutiliza `available_models`: lista vazia vira `SemListagem`
    /// (provider que não expõe catálogo), modelo presente vira `Listado` e
    /// ausente vira `Ausente`. Só quem tem dois níveis de catálogo (OpenRouter,
    /// com a curada de populares e a lista completa) precisa sobrescrever.
    async fn validar_modelo(&self, model: &str) -> Result<ValidacaoDeModelo> {
        let modelos = self.available_models().await?;
        if modelos.is_empty() {
            return Ok(ValidacaoDeModelo::SemListagem);
        }
        if modelos.iter().any(|m| m == model) {
            Ok(ValidacaoDeModelo::Listado)
        } else {
            Ok(ValidacaoDeModelo::Ausente)
        }
    }

    /// Verifica se o provedor está disponível e corretamente configurado.
    async fn health_check(&self) -> Result<bool>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub system: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f64>,
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: MessagePart,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessagePart {
    Text(String),
    Parts(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { url: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub content: Vec<ContentBlock>,
    pub model: String,
    pub usage: Option<Usage>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Eventos emitidos durante uma completion em modo streaming.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Um trecho incremental de texto gerado.
    TextDelta(String),
    /// Início de um bloco de uso de ferramenta.
    ToolUseStart {
        index: usize,
        id: String,
        name: String,
    },
    /// Fragmento parcial de JSON de entrada para um bloco de ferramenta.
    InputJsonDelta(String),
    /// Finalização de um bloco de conteúdo.
    ContentBlockStop { index: usize },
    /// Indica que a mensagem está sendo finalizada, incluindo metadados.
    MessageDelta {
        stop_reason: Option<String>,
        usage: Option<Usage>,
    },
    /// Indica que o streaming foi concluído.
    MessageStop,
}

// ── #1249: classificacao de falha de envio ───────────────────────────────────

/// `true` quando o `reqwest::Error` diz que a requisicao **nao chegou a ter
/// resposta**: conexao recusada, DNS que nao resolve, timeout, falha ao
/// enviar.
///
/// Fora de proposito:
/// - `is_builder()` — URL/cliente mal formado e bug de configuracao nosso, e
///   tentar outro provider esconderia o bug;
/// - `is_status()` — ja houve resposta, e a politica dela (429/5xx com
///   backoff) e a de sempre;
/// - `is_body()` / `is_decode()` — a conexao existiu e quebrou no meio do
///   corpo, caso do #1176, tratado no consumidor do stream.
///
/// A leitura e feita aqui, onde o tipo do `reqwest` ainda existe. Depois do
/// `format!` sobra texto, e texto de erro de dependencia muda de versao para
/// versao — era exatamente esse o defeito que a issue descreve.
pub(crate) fn falha_de_transporte(e: &reqwest::Error) -> bool {
    e.is_connect() || e.is_timeout() || e.is_request()
}

/// Erro de envio de requisicao a um provider, com a **classe** preservada.
///
/// `contexto` e o prefixo que o provider ja usava ("openai request failed"),
/// mantido palavra por palavra: o cartao de erro da CLI classifica pela frase
/// interna do `reqwest`, e nao pelo prefixo do enum.
pub(crate) fn erro_de_envio(contexto: &str, e: &reqwest::Error) -> Error {
    let msg = format!("{contexto}: {e}");
    if falha_de_transporte(e) {
        Error::Transport(msg)
    } else {
        Error::Agent(msg)
    }
}

// ── Cliente HTTP dos providers de LLM ────────────────────────────────────────

/// Prazo de conexao do cliente de LLM: um host morto tem de falhar rapido.
pub const CONNECT_TIMEOUT_LLM: std::time::Duration = std::time::Duration::from_secs(10);

/// Cliente HTTP para falar com um provider de LLM.
///
/// `inatividade` e o prazo maximo **sem nenhum byte chegando**, nao a duracao
/// total da resposta. `None` desliga o prazo (e o que `timeouts.llm.default_secs
/// = 0` quer dizer na config).
///
/// Ate aqui o bootstrap do gateway construia este cliente com
/// `ClientBuilder::timeout`, que no reqwest cobre a requisicao INTEIRA, leitura
/// do corpo inclusa. Uma resposta em streaming que demorasse mais que
/// `timeouts.llm.default_secs` (120 s por padrao) era cortada no meio mesmo
/// fluindo normalmente — loop agentico de varias voltas, ou o modelo local
/// default, um 27B de ~18 GB — e o corte chegava ao runtime como "stream read
/// error", sem nunca dizer que era um prazo. O alvo do prazo sempre foi
/// *provedor mudo*, e mudo e ausencia de bytes, nao tempo de parede:
/// `read_timeout` mede exatamente isso. Os providers dizem, nos dois,
/// "responses stream for minutes" — e este cliente, passado por
/// `with_client`, substitui o deles, entao tem de respeitar a mesma regra.
///
/// Redirects desligados, como no cliente default de cada provider (#1248,
/// regra 14): um endpoint de LLM nunca faz 302 legitimo para outro host, e
/// segui-lo contornaria o SSRF gate. O cliente antigo do bootstrap nao
/// desligava redirects, entao substituia um cliente que os recusava por um
/// que os seguia.
pub fn http_client_para_llm(inatividade: Option<std::time::Duration>) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT_LLM)
        .redirect(reqwest::redirect::Policy::none());
    if let Some(prazo) = inatividade {
        builder = builder.read_timeout(prazo);
    }
    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Servidor HTTP/1.1 de uma resposta so: manda os cabecalhos na hora e
    /// depois `pedacos` bytes, um por vez, dormindo `pausa` antes de cada um.
    /// Devolve a URL. E o formato de um provider vivo porem lento — e, com
    /// `pausa` maior que o prazo, de um provider mudo.
    async fn servidor_que_goteja(pedacos: usize, pausa: std::time::Duration) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind efemero");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            // Consome o pedido ate o fim dos cabecalhos; o conteudo nao importa.
            let mut buf = [0u8; 1024];
            let mut lido = Vec::new();
            loop {
                let n = socket.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                lido.extend_from_slice(&buf[..n]);
                if lido.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let cabecalho = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {pedacos}\r\nConnection: close\r\n\r\n"
            );
            socket
                .write_all(cabecalho.as_bytes())
                .await
                .expect("cabecalho");
            socket.flush().await.expect("flush");
            for _ in 0..pedacos {
                tokio::time::sleep(pausa).await;
                if socket.write_all(b"x").await.is_err() {
                    break;
                }
                let _ = socket.flush().await;
            }
        });
        format!("http://{addr}/")
    }

    /// Regressao do corte aos 120 s: uma resposta que demora mais que o prazo
    /// no TOTAL, mas nunca fica muda por um prazo inteiro, tem de chegar ao
    /// fim. O cliente antigo (`ClientBuilder::timeout`) a cortava — a segunda
    /// metade do teste prova que o contraste e real e nao folga do servidor.
    #[tokio::test]
    async fn resposta_lenta_porem_viva_nao_e_cortada() {
        let pausa = std::time::Duration::from_millis(100);
        let prazo = std::time::Duration::from_millis(400);
        // 8 pedacos x 100 ms = 800 ms de duracao total, o dobro do prazo.
        let url = servidor_que_goteja(8, pausa).await;
        let corpo = http_client_para_llm(Some(prazo))
            .get(&url)
            .send()
            .await
            .expect("cabecalhos chegam na hora")
            .bytes()
            .await
            .expect("resposta viva nao pode ser cortada por prazo de inatividade");
        assert_eq!(corpo.len(), 8, "os 8 pedacos tem de chegar");

        let url = servidor_que_goteja(8, pausa).await;
        let antigo = reqwest::Client::builder()
            .timeout(prazo)
            .build()
            .expect("cliente");
        let resultado = match antigo.get(&url).send().await {
            Ok(resposta) => resposta.bytes().await.map(|b| b.len()),
            Err(e) => Err(e),
        };
        let erro = resultado.expect_err("o prazo total antigo corta a resposta viva");
        assert!(erro.is_timeout(), "o corte antigo era um timeout: {erro}");
    }

    /// O outro lado da mesma regra: provider que emudece por um prazo inteiro
    /// ainda cai, e cai como transporte (timeout), que e o que o runtime
    /// trata com fallback.
    #[tokio::test]
    async fn provedor_mudo_cai_pelo_prazo_de_inatividade() {
        let prazo = std::time::Duration::from_millis(200);
        let url = servidor_que_goteja(1, std::time::Duration::from_secs(2)).await;
        let inicio = std::time::Instant::now();
        let resultado = match http_client_para_llm(Some(prazo)).get(&url).send().await {
            Ok(resposta) => resposta.bytes().await.map(|b| b.len()),
            Err(e) => Err(e),
        };
        let erro = resultado.expect_err("silencio de 2 s com prazo de 200 ms tem de falhar");
        assert!(
            erro.is_timeout(),
            "silencio e timeout, nao outra classe: {erro}"
        );
        assert!(
            falha_de_transporte(&erro),
            "o runtime so faz fallback do que e transporte: {erro}"
        );
        assert!(
            inicio.elapsed() < std::time::Duration::from_secs(2),
            "tem de cair pelo prazo, nao esperar o servidor terminar"
        );
    }

    /// Porta fechada em loopback: o unico erro de rede que um teste pode
    /// provocar sem depender de rede de verdade.
    async fn erro_de_conexao_recusada() -> reqwest::Error {
        // Abre e fecha para descobrir uma porta que ninguem esta servindo.
        let porta = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind efemero");
            l.local_addr().expect("addr").port()
        };
        reqwest::Client::new()
            .get(format!("http://127.0.0.1:{porta}/"))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
            .expect_err("ninguem esta escutando nessa porta")
    }

    #[tokio::test]
    async fn conexao_recusada_e_transporte() {
        let e = erro_de_conexao_recusada().await;
        assert!(
            falha_de_transporte(&e),
            "connect/timeout tem de ser transporte; veio: {e}"
        );
        assert!(
            matches!(
                erro_de_envio("openai request failed", &e),
                Error::Transport(_)
            ),
            "a classe precisa sobreviver ao format!"
        );
    }

    #[test]
    fn url_invalida_nao_e_transporte() {
        // `is_builder()`: nada saiu da maquina, e cair para outro provider
        // esconderia um bug de configuracao nosso.
        let e = reqwest::Client::new()
            .get("http://[::1")
            .build()
            .expect_err("url invalida");
        assert!(!falha_de_transporte(&e), "builder nao e transporte: {e}");
        assert!(matches!(
            erro_de_envio("openai request failed", &e),
            Error::Agent(_)
        ));
    }

    #[tokio::test]
    async fn mensagem_do_provider_sobrevive_inteira() {
        let e = erro_de_conexao_recusada().await;
        let texto = erro_de_envio("ollama request failed", &e).to_string();
        assert!(
            texto.contains("ollama request failed"),
            "o prefixo do provider fica; veio: {texto}"
        );
        assert!(
            texto.starts_with("transport error: "),
            "e o prefixo da classe entra na frente; veio: {texto}"
        );
    }
}
