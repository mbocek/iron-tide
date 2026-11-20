# Dedicated server (planned)

Network battles are hosted today by one player's game. The plan is a dedicated server on the internet that every
player connects to, so nobody has to forward a port and one player's computer dropping out does not end the
battle. Nothing is split out yet; this page records what is ready in the game and what separating the server
takes.

## Why it fits

The simulation is deterministic lockstep ([determinism.md](determinism.md)): every machine runs the whole battle
and they exchange only orders. The host's part of a network battle is therefore pure message handling, which
`Relay` (`src/relay.rs`) does without touching the simulation:

- passing each guest's turns, checksums and chat on to every other machine;
- answering pings;
- remembering every order (`TurnHistory`: each team's turns, complete through their latest tick, listing only
  those with orders);
- keeping its port open during the battle: a late guest is offered the battle to watch, or the seat of a player
  whose connection dropped, and on accepting gets the whole history and catches up at 40x speed;
- holding the battle up to three minutes for a dropped player, and ending it after that or when a player drops
  with no way back.

A server is the same `Relay` with no simulation of its own.

## Ready in the game

- **Transport.** Every link is a WebSocket (`tungstenite`), each message one binary frame of JSON
  (`Message` in `src/network.rs`, public so a server binary in this crate can use it). The host answers the
  handshake, a guest opens it.
- **Addresses with rooms.** The join field takes `host`, `host:port` or `ws://host[:port]/path`
  (`JoinTarget`). The path is sent in the WebSocket request; the host in the game ignores it, a server will
  read it as the room.
- **The hub.** `Relay` holds the links, which of them carry players and which spectators, the history and the
  open door. `NetSession` keeps only what this machine's simulation needs: turns due, checksums, ping, chat.
- **Catching up and rejoining**, tested headless in `tests/network.rs` over real WebSocket connections.

## Separating it

1. **Server binary.** `src/bin/server.rs` in this crate, using the library without starting Bevy: accept
   WebSocket connections, read the room from the request path (`tungstenite::accept_hdr` with a callback), and
   give each room its own `Relay`.
2. **Lobby on the server.** Before the battle the host's seats, offers, *I'm ready* and *Go* live in
   `src/lobby.rs` (`LobbyPhase::Hosting`). They move into the room: the player who creates it picks the battle
   (the battle editor's settings, as `LevelDefinition`), the server offers it to each guest and starts the
   countdown when every seat is ready. The room code is the path, e.g. `ws://play.example.org/room/K7Q2`.
3. **Every player is a guest.** With the server as hub, the room creator's game connects like everyone else and
   plays `Team::Player`; `NetSession::new(.., relays: false)` already does this.
4. **Checksums.** Today each machine compares the checksums it receives with its own. The server sees all of
   them and can decide alone, telling every machine which tick went out of step.
5. **Secure connections.** For `wss://` the game needs TLS on the client side: tungstenite's
   `rustls-tls-webpki-roots` feature (pulls in rustls and a crypto backend, to check against the Windows and
   macOS cross builds in CI) and dialling through it in `Link`. The server itself can stay plain `ws://` behind a
   proxy that ends TLS.
6. **Limits.** Rooms per server, players and spectators per room, the largest message (16 MB, set by the
   catch-up history), and idle rooms closed after a while.

## Deployment sketch

The server is one binary listening on one port; a reverse proxy adds TLS and a certificate:

```dockerfile
FROM rust:1.100 AS build
WORKDIR /src
COPY . .
RUN cargo build --release --bin iron-tide-server

FROM debian:stable-slim
COPY --from=build /src/target/release/iron-tide-server /usr/local/bin/
EXPOSE 47300
CMD ["iron-tide-server", "--port", "47300"]
```

```caddyfile
play.example.org {
    reverse_proxy iron-tide-server:47300
}
```

Caddy passes WebSocket upgrades through by default, so players join `wss://play.example.org/room/K7Q2`.

## Open decisions

- Whether rooms are listed on the server or joined only by code.
- Whether the server arbitrates out-of-step battles or only relays.
- Where it runs and who may create rooms.
- Whether local hosting from the game stays alongside the server (it costs nothing to keep).
