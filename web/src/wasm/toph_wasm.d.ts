/* tslint:disable */
/* eslint-disable */
/**
 * The `ReadableStreamType` enum.
 *
 * *This API requires the following crate features to be activated: `ReadableStreamType`*
 */

export type ReadableStreamType = "bytes";

export class IntoUnderlyingByteSource {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    cancel(): void;
    pull(controller: ReadableByteStreamController): Promise<any>;
    start(controller: ReadableByteStreamController): void;
    readonly autoAllocateChunkSize: number;
    readonly type: ReadableStreamType;
}

export class IntoUnderlyingSink {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    abort(reason: any): Promise<any>;
    close(): Promise<any>;
    write(chunk: any): Promise<any>;
}

export class IntoUnderlyingSource {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    cancel(): void;
    pull(controller: ReadableStreamDefaultController): Promise<any>;
}

export class TophCall {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    hang_up(): void;
    /**
     * `callback(data: Uint8Array, timestampUs: number)`
     */
    on_audio(cb: Function): void;
    /**
     * `callback()` — fires at most once when the call ends for any reason.
     */
    on_close(cb: Function): void;
    /**
     * `callback()`
     */
    on_keyframe_request(cb: Function): void;
    /**
     * `callback(data: Uint8Array, timestampUs: number, isKey: boolean)`
     */
    on_video(cb: Function): void;
    remote_height(): number;
    remote_node_id(): string;
    remote_width(): number;
    request_keyframe(): void;
    send_audio(data: Uint8Array, timestamp_us: number): void;
    send_video(data: Uint8Array, timestamp_us: number, is_key: boolean): void;
}

/**
 * A pending incoming call. Call `accept` or `reject` exactly once.
 */
export class TophIncomingCall {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Accept the call and perform the media handshake.
     * `width`/`height` are the dimensions of the video *we* will send.
     */
    accept(width: number, height: number): Promise<TophCall>;
    /**
     * Reject the call.
     */
    reject(): Promise<void>;
}

export class TophSession {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Returns a JSON string with detailed path diagnostics, or "" if no info yet.
     */
    connection_debug_info(node_id_hex: string): Promise<string>;
    /**
     * Returns "direct", "relay", or "unknown" for the active path to `node_id_hex`.
     */
    connection_type(node_id_hex: string): Promise<string>;
    /**
     * Bind an iroh endpoint and wait for the relay connection to come up.
     */
    static create(): Promise<TophSession>;
    /**
     * Dial the peer identified by a 64-char hex ticket.
     * Sends a Ring and waits for Accept/Reject.
     * Returns `TophCall` on accept, `null` if the remote rejected.
     */
    dial(ticket: string, width: number, height: number): Promise<TophCall | undefined>;
    /**
     * Returns the 64-char hex node ID. Share this with a peer so they can dial you.
     */
    ticket(): Promise<string>;
    /**
     * Wait for the next incoming connection and return an `IncomingCall`
     * that the user can accept or reject.
     */
    wait_for_ring(): Promise<TophIncomingCall>;
}

export function init(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_tophcall_free: (a: number, b: number) => void;
    readonly __wbg_tophincomingcall_free: (a: number, b: number) => void;
    readonly __wbg_tophsession_free: (a: number, b: number) => void;
    readonly init: () => void;
    readonly tophcall_hang_up: (a: number) => void;
    readonly tophcall_on_audio: (a: number, b: any) => void;
    readonly tophcall_on_close: (a: number, b: any) => void;
    readonly tophcall_on_keyframe_request: (a: number, b: any) => void;
    readonly tophcall_on_video: (a: number, b: any) => void;
    readonly tophcall_remote_height: (a: number) => number;
    readonly tophcall_remote_node_id: (a: number) => [number, number];
    readonly tophcall_remote_width: (a: number) => number;
    readonly tophcall_request_keyframe: (a: number) => void;
    readonly tophcall_send_audio: (a: number, b: number, c: number, d: number) => void;
    readonly tophcall_send_video: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly tophincomingcall_accept: (a: number, b: number, c: number) => any;
    readonly tophincomingcall_reject: (a: number) => any;
    readonly tophsession_connection_debug_info: (a: number, b: number, c: number) => any;
    readonly tophsession_connection_type: (a: number, b: number, c: number) => any;
    readonly tophsession_create: () => any;
    readonly tophsession_dial: (a: number, b: number, c: number, d: number, e: number) => any;
    readonly tophsession_ticket: (a: number) => any;
    readonly tophsession_wait_for_ring: (a: number) => any;
    readonly __wbg_intounderlyingsink_free: (a: number, b: number) => void;
    readonly intounderlyingsink_abort: (a: number, b: any) => any;
    readonly intounderlyingsink_close: (a: number) => any;
    readonly intounderlyingsink_write: (a: number, b: any) => any;
    readonly __wbg_intounderlyingsource_free: (a: number, b: number) => void;
    readonly intounderlyingsource_cancel: (a: number) => void;
    readonly intounderlyingsource_pull: (a: number, b: any) => any;
    readonly __wbg_intounderlyingbytesource_free: (a: number, b: number) => void;
    readonly intounderlyingbytesource_autoAllocateChunkSize: (a: number) => number;
    readonly intounderlyingbytesource_cancel: (a: number) => void;
    readonly intounderlyingbytesource_pull: (a: number, b: any) => any;
    readonly intounderlyingbytesource_start: (a: number, b: any) => void;
    readonly intounderlyingbytesource_type: (a: number) => number;
    readonly ring_core_0_17_14__bn_mul_mont: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly wasm_bindgen_f3f829865fba9763___convert__closures_____invoke___wasm_bindgen_f3f829865fba9763___JsValue__core_7d5f0a2ba6a62c33___result__Result_____wasm_bindgen_f3f829865fba9763___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_f3f829865fba9763___convert__closures_____invoke___js_sys_a7b70c87c41ecf83___Function_fn_wasm_bindgen_f3f829865fba9763___JsValue_____wasm_bindgen_f3f829865fba9763___sys__Undefined___js_sys_a7b70c87c41ecf83___Function_fn_wasm_bindgen_f3f829865fba9763___JsValue_____wasm_bindgen_f3f829865fba9763___sys__Undefined_______true_: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_f3f829865fba9763___convert__closures_____invoke___wasm_bindgen_f3f829865fba9763___JsValue______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_f3f829865fba9763___convert__closures_____invoke___web_sys_7e6b3c2247d1d7d0___features__gen_CloseEvent__CloseEvent______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_f3f829865fba9763___convert__closures_____invoke___web_sys_7e6b3c2247d1d7d0___features__gen_MessageEvent__MessageEvent______true_: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen_f3f829865fba9763___convert__closures_____invoke_______true_: (a: number, b: number) => void;
    readonly wasm_bindgen_f3f829865fba9763___convert__closures_____invoke_______true__1_: (a: number, b: number) => void;
    readonly wasm_bindgen_f3f829865fba9763___convert__closures_____invoke_______true__2_: (a: number, b: number) => void;
    readonly wasm_bindgen_f3f829865fba9763___convert__closures_____invoke_______true__3_: (a: number, b: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
