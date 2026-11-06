# BAS ACP multiplexing

Agreed implementation contract, 2026-11-06.

The [event-stream separation contract](agents-event-streams.md) defines the distinct ACP and BAS management streams and takes precedence over references below to shared session events.

BAS is the persistent ACP client of each Agent. Desktop and remote BAS frontends are independent producers and observers of that client's session stream. An Agent session does not have an exclusive frontend controller.

## Input and observation

- Submission and observation are separate operations. BAS multiplexes frontend inputs into its Agent connection and distributes session events to all subscribers.
- BAS assigns the outgoing ACP request ID. The submission receipt exposes that same ID, allowing observers to associate the input with its eventual response. An ACP request ID identifies a request within its connection; it is not a journal cursor.
- A successful receipt confirms admission to the BAS-managed ACP stream, not successful Agent execution. Agent errors are observable outcomes associated with the request.
- The frontend renders conversation messages from the observed stream only. It must not insert a provisional conversation message from the submission or its receipt.
- An observed input may arrive before its submission receipt. Observation and admission confirmation proceed independently; no frontend reconciliation mechanism is needed to render the conversation.
- Streamed session notifications need not contain an ACP request ID. Retain existing session/turn associations where needed; do not invent request associations for session-wide events.
- Preserve existing durable history and replay mechanisms. Transport request IDs and history cursors serve different purposes.

## Routing and lifetime

- Source connections have independent request-ID namespaces. BAS routes direct responses to the originating caller and publishes shared session changes to subscribers.
- Shared state includes admitted user messages, Agent output, execution outcomes, configuration changes, and pending/resolved interactions. A frontend's private reads and submission failures do not create conversation messages.
- Agent-originated interactive requests may be delivered to multiple eligible frontends. Forward the first valid reply once and retire the other deliveries. This is request-level completion, not session ownership.
- Frontend disconnection removes only that frontend's subscriptions and delivery paths. It does not cancel Agent execution or resolve a pending Agent request as cancelled.
- Explicit user cancellation remains an ordinary command. Agent-connection termination and BAS instance reclamation follow their existing lifecycle policies separately.
- Unsupported frontend interaction capabilities do not cancel a shared request for other frontends.

## Scope

Do not introduce a primary frontend, controller election, preemption, controller leases, or an additional command scheduler. Forward supported commands and preserve Agent rejection semantics; use existing session execution constraints. Keep existing authentication and connection-local callback validation.

## Verification

Verify two frontends observing one session, one frontend leaving while work or an interaction remains pending, duplicate interaction replies, and conversation rendering driven by observed inputs regardless of receipt ordering. Verify request IDs exposed in receipts match the requests sent to the Agent. Run focused existing backend and frontend tests and compile affected targets.
