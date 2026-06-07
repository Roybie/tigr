// tigr's web WebSocket backend: a reference miniquad JS plugin that
// implements the `tigr_ws_*` env imports the `WS` module declares on a
// plain-wasm (non-wasm-bindgen) host. It is the JS half of the FFI
// contract in src/vm/native_modules/ws_web.rs; the Rust half declares
// the matching `env` imports and keys every connection by the same
// integer handle.
//
// Drop this into a plain-wasm host (purr's miniquad loader) so that
// `import 'WS'` works in the browser with the same API as native. On a
// host that does not load it, miniquad stubs the imports and `WS` calls
// no-op (state reads back as closed), so the build still runs.
//
// Loaded after the miniquad JS bundle (which defines miniquad_add_plugin
// and the globals wasm_memory / wasm_exports) and before the wasm loads.
//
// ABI (see docs/stdlib/ws.md):
//   tigr_ws_connect(ptr,len) -> i32 handle (>=0), or <0 on a sync reject
//   tigr_ws_send(id, ptr, len, is_binary)
//   tigr_ws_poll(id, out, cap) -> framed length, or 0 none, or <0 closed
//   tigr_ws_state(id) -> 0 connecting | 1 open | 2 closed
//   tigr_ws_close(id)
// A polled message is one tag byte (0 text, 1 binary) then the payload.
"use strict";

(function () {
    const sockets = new Map();   // id -> { ws, queue: [{tag,bytes}], closed }
    let nextId = 1;

    function u8() {
        return new Uint8Array(wasm_memory.buffer);
    }

    function readUtf8(ptr, len) {
        return new TextDecoder("utf-8").decode(
            new Uint8Array(wasm_memory.buffer, ptr, len));
    }

    miniquad_add_plugin({
        name: "tigr_ws",
        register_plugin: function (importObject) {
            // Open a connection. Returns a non-negative handle; the
            // socket connects asynchronously, so failure surfaces later
            // through tigr_ws_state, not here.
            importObject.env.tigr_ws_connect = function (ptr, len) {
                const url = readUtf8(ptr, len);
                let ws;
                try {
                    ws = new WebSocket(url);
                } catch (e) {
                    return -1;
                }
                ws.binaryType = "arraybuffer";
                const id = nextId++;
                const rec = { ws: ws, queue: [], closed: false };
                sockets.set(id, rec);
                ws.onmessage = function (ev) {
                    if (typeof ev.data === "string") {
                        rec.queue.push({ tag: 0, bytes: new TextEncoder().encode(ev.data) });
                    } else {
                        rec.queue.push({ tag: 1, bytes: new Uint8Array(ev.data) });
                    }
                };
                ws.onclose = function () { rec.closed = true; };
                ws.onerror = function () { rec.closed = true; };
                return id;
            };

            // Send one message. is_binary selects a binary or text frame.
            importObject.env.tigr_ws_send = function (id, ptr, len, is_binary) {
                const rec = sockets.get(id);
                if (!rec || rec.ws.readyState !== WebSocket.OPEN) {
                    return;
                }
                // Copy out of linear memory: the buffer can detach on a
                // later allocation, so never hand the live view to send().
                const bytes = u8().slice(ptr, ptr + len);
                if (is_binary) {
                    rec.ws.send(bytes);
                } else {
                    rec.ws.send(new TextDecoder("utf-8").decode(bytes));
                }
            };

            // Dequeue the next inbound message into out/cap. See the ABI
            // note above for the return-code contract.
            importObject.env.tigr_ws_poll = function (id, out, cap) {
                const rec = sockets.get(id);
                if (!rec) {
                    return -1;
                }
                if (rec.queue.length === 0) {
                    return rec.closed ? -1 : 0;
                }
                const msg = rec.queue[0];
                const framed = 1 + msg.bytes.length;
                if (framed > cap) {
                    return framed;          // too small; keep the message
                }
                const mem = u8();
                mem[out] = msg.tag;
                mem.set(msg.bytes, out + 1);
                rec.queue.shift();
                return framed;
            };

            // 0 connecting, 1 open, 2 closed (or unknown handle).
            importObject.env.tigr_ws_state = function (id) {
                const rec = sockets.get(id);
                if (!rec) {
                    return 2;
                }
                switch (rec.ws.readyState) {
                    case WebSocket.CONNECTING: return 0;
                    case WebSocket.OPEN: return 1;
                    default: return 2;
                }
            };

            importObject.env.tigr_ws_close = function (id) {
                const rec = sockets.get(id);
                if (!rec) {
                    return;
                }
                rec.closed = true;
                try { rec.ws.close(); } catch (e) { /* already closing */ }
            };
        },
    });
})();
