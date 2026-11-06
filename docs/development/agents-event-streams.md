# BAS management events and ACP observation

Agreed architecture, 2026-11-06. This document refines
[ACP multiplexing](agents-acp-multiplexing.md).

## Ownership

BAS is the persistent client of the Agent. Frontends submit inputs to BAS and
observe its protocol exchanges. Frontend attachment never owns Agent execution.

There are two independent event streams:

- **ACP observation** carries protocol requests, responses and notifications.
  It drives live conversation content and protocol interactions. The outgoing
  prompt request ID is the ID returned by submission admission. BAS does not
  replace an Agent response with an application lifecycle event.
- **BAS management** carries BAS-owned Session state, titles managed by BAS,
  archival/workspace changes, instance lifecycle and service failures. Agent
  Session state managed or derived by BAS remains application state, even when
  an ACP exchange caused the transition. It never becomes an ACP notification.

An Agent error response belongs to ACP. A dispatch failure, lost Agent process,
or BAS restart belongs to BAS. A management failure can mark an interrupted
conversation in the UI without fabricating an Agent response.

## History and live rendering

History is an on-demand query, used on opening, restoration and pagination.
It supplies an existing conversation window and its necessary context. The
frontend restores that window separately from applying live ACP increments.
The existing event-backed history representation may remain an implementation
detail; this change does not require a new rendered-block storage format.

History returns the position through which the window is complete. Observation
continues after that position, including messages produced during the query.
An in-progress message/tool must retain its identity across this boundary.
Pagination and observation positions belong to the transport/storage envelope;
they are neither ACP request IDs nor custom ACP messages.

Each stream advances independently. A newer management event must not cause an
older, still-in-transit ACP message to be discarded, or vice versa. Reconnection
must preserve both frontiers. Historical permission requests are observations,
not instructions to execute callbacks again.

## Implementation constraints

- Do not stream conversation chunks/results through the BAS management stream.
- Do not inject BAS Session state or service errors into ACP observation.
- Do not render submitted content optimistically from an admission receipt.
- Preserve history pagination, callback authorization, first-valid-response
  handling, and the existing Agent instance reclamation policy.
- Do not add frontend ownership, controller election, preemption or scheduling.

## Verification

Check stream separation, request/response identity, two observers, independent
stream arrival order, history/live continuity and reconnects. Check that a BAS
failure remains distinguishable from an Agent error, and that disconnecting a
frontend does not cancel Agent work or pending interactions.

## Implemented interfaces

- `Bas.ObserveAcp` delivers JSON-RPC prompt requests/results and session update
  notifications. Its envelope carries the public session identity, observation
  position, timestamp and optional submission association. Request IDs match
  the reserved Agent request IDs. Session IDs use the public BAS namespace,
  consistent with the existing ACP adapter.
- `Bas.Attach` delivers management events only. Prompt admission/completion is
  reflected as application activity state, without copying the protocol result.
- Commands and permission/elicitation callbacks continue on `AcpTransport`.
  Frontends using `ObserveAcp` prepare this connection with `callbacks_only`,
  avoiding a duplicate history replay and duplicate live content delivery.
- Existing history queries remain finite and paginated. Storage may retain a
  common ordered journal internally; its presentation records are not the
  management wire contract. Input chunks derived for historical presentation
  are not re-emitted as fabricated live Agent notifications.

The frontend has separate history restoration, live protocol rendering and
management-update entry points. Management updates never advance the content
renderer cursor. Live protocol rendering does not set BAS Session activity.
The local cache advances only across complete history ranges; batches from the
independent streams are sorted before the existing contiguous-range check.

This protocol change requires the matching BAS and frontend builds. Existing
history remains readable. Old records without recorded prompt JSON can still
be restored through history queries; no old-format live-observation fallback
or data migration is introduced.
