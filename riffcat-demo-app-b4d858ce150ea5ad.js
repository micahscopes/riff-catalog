/**
 * A perceptually-uniform chip color for a shape digest, as a CSS `oklch(...)`
 * string: deterministic in the digest (same shape, same color in every
 * chapter) and gamut-safe (always renders).
 * @param {string} digest_hex
 * @returns {string}
 */
export function chip_color(digest_hex) {
    let deferred2_0;
    let deferred2_1;
    try {
        const ptr0 = passStringToWasm0(digest_hex, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.chip_color(ptr0, len0);
        deferred2_0 = ret[0];
        deferred2_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred2_0, deferred2_1, 1);
    }
}

/**
 * Fingerprint a chord written as a real notation string. Parses it with the
 * vibe-grammars pest parser, derives its pitch-class set, and returns the
 * note-set facet address (plus full). A real string parsed to a structural
 * fingerprint: the music analogue of parsing Solidity source.
 * @param {string} notation
 * @returns {string}
 */
export function fingerprint_chord(notation) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(notation, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.fingerprint_chord(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Fingerprint a raw pitch-class set, optionally inverting it first by the I
 * generator (x -> (12 - x) mod 12). This lets the demo address a set it built
 * itself (an inverted chord) through the same engine that handled the parsed
 * chord, so the Tn-type, prime form, and interval vector on screen are always
 * computed, never reimplemented in JavaScript. `pcs_json` is a JSON array of
 * integers.
 * @param {string} pcs_json
 * @param {boolean} invert
 * @returns {string}
 */
export function fingerprint_pcs(pcs_json, invert) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(pcs_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.fingerprint_pcs(ptr0, len0, invert);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Fingerprint a melodic riff (and its pitch-class set). This is the engine on a
 * second, audible domain: the very same facet machinery, no Solidity in sight.
 * Input is `{"notes":[{"pitch":<midi int>,"dur":<uint>}, ...]}`; output is the
 * "equal at facet" address for each musical facet, so the storybook can group
 * riffs by shape the way it groups code: rhythm (durations), harmonic
 * relationships (intervals, transposition invariant), pitch-class set, and full.
 * @param {string} riff_json
 * @returns {string}
 */
export function fingerprint_riff(riff_json) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(riff_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.fingerprint_riff(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

/**
 * Fingerprint a contract's Solidity source. Input is solc's source `ast` JSON;
 * output is the source unit plus one fingerprint per contract and function.
 * This is the version-stable level (the source AST does not drift with the
 * compiler), so it is the right substrate for "same library code".
 * @param {string} ast_json
 * @param {string} mode
 * @returns {string}
 */
export function fingerprint_source(ast_json, mode) {
    let deferred4_0;
    let deferred4_1;
    try {
        const ptr0 = passStringToWasm0(ast_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(mode, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.fingerprint_source(ptr0, len0, ptr1, len1);
        var ptr3 = ret[0];
        var len3 = ret[1];
        if (ret[3]) {
            ptr3 = 0; len3 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred4_0 = ptr3;
        deferred4_1 = len3;
        return getStringFromWasm0(ptr3, len3);
    } finally {
        wasm.__wbindgen_free(deferred4_0, deferred4_1, 1);
    }
}

/**
 * Fingerprint a contract's Yul. Input is solc's `irAst` JSON for one object
 * (the via-IR AST); output is a JSON array of unit fingerprints (the object
 * graph plus one per function). `mode` is "shape" (default) or "identity".
 * @param {string} ir_ast_json
 * @param {string} mode
 * @returns {string}
 */
export function fingerprint_yul(ir_ast_json, mode) {
    let deferred4_0;
    let deferred4_1;
    try {
        const ptr0 = passStringToWasm0(ir_ast_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(mode, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.fingerprint_yul(ptr0, len0, ptr1, len1);
        var ptr3 = ret[0];
        var len3 = ret[1];
        if (ret[3]) {
            ptr3 = 0; len3 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred4_0 = ptr3;
        deferred4_1 = len3;
        return getStringFromWasm0(ptr3, len3);
    } finally {
        wasm.__wbindgen_free(deferred4_0, deferred4_1, 1);
    }
}

/**
 * The raw solc JSON (irAst or source AST) for one fixture id, or `null`.
 * @param {string} id
 * @returns {string | undefined}
 */
export function fixture(id) {
    const ptr0 = passStringToWasm0(id, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
    const len0 = WASM_VECTOR_LEN;
    const ret = wasm.fixture(ptr0, len0);
    let v2;
    if (ret[0] !== 0) {
        v2 = getStringFromWasm0(ret[0], ret[1]).slice();
        wasm.__wbindgen_free(ret[0], ret[1] * 1, 1);
    }
    return v2;
}

/**
 * JSON array of `{id, label, level}` for every baked fixture, for the picker.
 * @returns {string}
 */
export function fixture_index() {
    let deferred1_0;
    let deferred1_1;
    try {
        const ret = wasm.fixture_index();
        deferred1_0 = ret[0];
        deferred1_1 = ret[1];
        return getStringFromWasm0(ret[0], ret[1]);
    } finally {
        wasm.__wbindgen_free(deferred1_0, deferred1_1, 1);
    }
}

/**
 * Surface the per-node digests `digest_graph` computes for one lowered unit, for
 * the "the fold" chapter. Input is solc's `irAst` JSON for one object (same as
 * `fingerprint_yul`); it lowers the object and folds one small fn-like unit (see
 * `node_digests_impl` for the selection), so the payload reads as a small tree
 * with statements, an expression, and leaves. `mode` is "shape" (default,
 * anonymous) or "identity", exactly as the other exports. Returns the JSON the
 * `fold-merkle` element consumes: `nodes` (with `local`/`tree` per dimension),
 * `components`, and the `graph` digest.
 * @param {string} ir_ast_json
 * @param {string} mode
 * @returns {string}
 */
export function node_digests(ir_ast_json, mode) {
    let deferred4_0;
    let deferred4_1;
    try {
        const ptr0 = passStringToWasm0(ir_ast_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(mode, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ret = wasm.node_digests(ptr0, len0, ptr1, len1);
        var ptr3 = ret[0];
        var len3 = ret[1];
        if (ret[3]) {
            ptr3 = 0; len3 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred4_0 = ptr3;
        deferred4_1 = len3;
        return getStringFromWasm0(ptr3, len3);
    } finally {
        wasm.__wbindgen_free(deferred4_0, deferred4_1, 1);
    }
}

/**
 * Compare two lowerings of the same source at a dimension. Returns the engine's
 * containment fraction each way (how much of one shape's subtrees are present in
 * the other) and, engine-derived, exactly which nodes diverge (the subtree
 * digests in one that are absent from the other). The page reads a differing
 * node's recorded origin to say where it came from; the verdict of which nodes
 * differ is the engine's. `dimension` is one of the closed dimension names,
 * e.g. `structure`.
 * @param {string} a_json
 * @param {string} b_json
 * @param {string} dimension
 * @returns {string}
 */
export function origin_containment(a_json, b_json, dimension) {
    let deferred5_0;
    let deferred5_1;
    try {
        const ptr0 = passStringToWasm0(a_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ptr1 = passStringToWasm0(b_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len1 = WASM_VECTOR_LEN;
        const ptr2 = passStringToWasm0(dimension, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len2 = WASM_VECTOR_LEN;
        const ret = wasm.origin_containment(ptr0, len0, ptr1, len1, ptr2, len2);
        var ptr4 = ret[0];
        var len4 = ret[1];
        if (ret[3]) {
            ptr4 = 0; len4 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred5_0 = ptr4;
        deferred5_1 = len4;
        return getStringFromWasm0(ptr4, len4);
    } finally {
        wasm.__wbindgen_free(deferred5_0, deferred5_1, 1);
    }
}

/**
 * Digest one lowering twice, with the `EdgeRole::Origin` provenance edges
 * present and stripped, and return both facet addresses (structure and full)
 * plus the node count and edge tallies. The structural address is identical
 * either way, computed by the engine both times: provenance rides along as
 * payload and does not move the shape the catalog dedups on. Input is the
 * compact `OriginSpec` JSON the page assembles.
 * @param {string} spec_json
 * @returns {string}
 */
export function origin_shape(spec_json) {
    let deferred3_0;
    let deferred3_1;
    try {
        const ptr0 = passStringToWasm0(spec_json, wasm.__wbindgen_malloc, wasm.__wbindgen_realloc);
        const len0 = WASM_VECTOR_LEN;
        const ret = wasm.origin_shape(ptr0, len0);
        var ptr2 = ret[0];
        var len2 = ret[1];
        if (ret[3]) {
            ptr2 = 0; len2 = 0;
            throw takeFromExternrefTable0(ret[2]);
        }
        deferred3_0 = ptr2;
        deferred3_1 = len2;
        return getStringFromWasm0(ptr2, len2);
    } finally {
        wasm.__wbindgen_free(deferred3_0, deferred3_1, 1);
    }
}

function __wbg_get_imports() {
    const import0 = {
        __proto__: null,
        __wbindgen_cast_0000000000000001: function(arg0, arg1) {
            // Cast intrinsic for `Ref(String) -> Externref`.
            const ret = getStringFromWasm0(arg0, arg1);
            return ret;
        },
        __wbindgen_init_externref_table: function() {
            const table = wasm.__wbindgen_externrefs;
            const offset = table.grow(4);
            table.set(0, undefined);
            table.set(offset + 0, undefined);
            table.set(offset + 1, null);
            table.set(offset + 2, true);
            table.set(offset + 3, false);
        },
    };
    return {
        __proto__: null,
        "./riffcat-demo-app_bg.js": import0,
    };
}

function getStringFromWasm0(ptr, len) {
    ptr = ptr >>> 0;
    return decodeText(ptr, len);
}

let cachedUint8ArrayMemory0 = null;
function getUint8ArrayMemory0() {
    if (cachedUint8ArrayMemory0 === null || cachedUint8ArrayMemory0.byteLength === 0) {
        cachedUint8ArrayMemory0 = new Uint8Array(wasm.memory.buffer);
    }
    return cachedUint8ArrayMemory0;
}

function passStringToWasm0(arg, malloc, realloc) {
    if (realloc === undefined) {
        const buf = cachedTextEncoder.encode(arg);
        const ptr = malloc(buf.length, 1) >>> 0;
        getUint8ArrayMemory0().subarray(ptr, ptr + buf.length).set(buf);
        WASM_VECTOR_LEN = buf.length;
        return ptr;
    }

    let len = arg.length;
    let ptr = malloc(len, 1) >>> 0;

    const mem = getUint8ArrayMemory0();

    let offset = 0;

    for (; offset < len; offset++) {
        const code = arg.charCodeAt(offset);
        if (code > 0x7F) break;
        mem[ptr + offset] = code;
    }
    if (offset !== len) {
        if (offset !== 0) {
            arg = arg.slice(offset);
        }
        ptr = realloc(ptr, len, len = offset + arg.length * 3, 1) >>> 0;
        const view = getUint8ArrayMemory0().subarray(ptr + offset, ptr + len);
        const ret = cachedTextEncoder.encodeInto(arg, view);

        offset += ret.written;
        ptr = realloc(ptr, len, offset, 1) >>> 0;
    }

    WASM_VECTOR_LEN = offset;
    return ptr;
}

function takeFromExternrefTable0(idx) {
    const value = wasm.__wbindgen_externrefs.get(idx);
    wasm.__externref_table_dealloc(idx);
    return value;
}

let cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
cachedTextDecoder.decode();
const MAX_SAFARI_DECODE_BYTES = 2146435072;
let numBytesDecoded = 0;
function decodeText(ptr, len) {
    numBytesDecoded += len;
    if (numBytesDecoded >= MAX_SAFARI_DECODE_BYTES) {
        cachedTextDecoder = new TextDecoder('utf-8', { ignoreBOM: true, fatal: true });
        cachedTextDecoder.decode();
        numBytesDecoded = len;
    }
    return cachedTextDecoder.decode(getUint8ArrayMemory0().subarray(ptr, ptr + len));
}

const cachedTextEncoder = new TextEncoder();

if (!('encodeInto' in cachedTextEncoder)) {
    cachedTextEncoder.encodeInto = function (arg, view) {
        const buf = cachedTextEncoder.encode(arg);
        view.set(buf);
        return {
            read: arg.length,
            written: buf.length
        };
    };
}

let WASM_VECTOR_LEN = 0;

let wasmModule, wasm;
function __wbg_finalize_init(instance, module) {
    wasm = instance.exports;
    wasmModule = module;
    cachedUint8ArrayMemory0 = null;
    wasm.__wbindgen_start();
    return wasm;
}

async function __wbg_load(module, imports) {
    if (typeof Response === 'function' && module instanceof Response) {
        if (typeof WebAssembly.instantiateStreaming === 'function') {
            try {
                return await WebAssembly.instantiateStreaming(module, imports);
            } catch (e) {
                const validResponse = module.ok && expectedResponseType(module.type);

                if (validResponse && module.headers.get('Content-Type') !== 'application/wasm') {
                    console.warn("`WebAssembly.instantiateStreaming` failed because your server does not serve Wasm with `application/wasm` MIME type. Falling back to `WebAssembly.instantiate` which is slower. Original error:\n", e);

                } else { throw e; }
            }
        }

        const bytes = await module.arrayBuffer();
        return await WebAssembly.instantiate(bytes, imports);
    } else {
        const instance = await WebAssembly.instantiate(module, imports);

        if (instance instanceof WebAssembly.Instance) {
            return { instance, module };
        } else {
            return instance;
        }
    }

    function expectedResponseType(type) {
        switch (type) {
            case 'basic': case 'cors': case 'default': return true;
        }
        return false;
    }
}

function initSync(module) {
    if (wasm !== undefined) return wasm;


    if (module !== undefined) {
        if (Object.getPrototypeOf(module) === Object.prototype) {
            ({module} = module)
        } else {
            console.warn('using deprecated parameters for `initSync()`; pass a single object instead')
        }
    }

    const imports = __wbg_get_imports();
    if (!(module instanceof WebAssembly.Module)) {
        module = new WebAssembly.Module(module);
    }
    const instance = new WebAssembly.Instance(module, imports);
    return __wbg_finalize_init(instance, module);
}

async function __wbg_init(module_or_path) {
    if (wasm !== undefined) return wasm;


    if (module_or_path !== undefined) {
        if (Object.getPrototypeOf(module_or_path) === Object.prototype) {
            ({module_or_path} = module_or_path)
        } else {
            console.warn('using deprecated parameters for the initialization function; pass a single object instead')
        }
    }

    if (module_or_path === undefined) {
        module_or_path = new URL('riffcat-demo-app_bg.wasm', import.meta.url);
    }
    const imports = __wbg_get_imports();

    if (typeof module_or_path === 'string' || (typeof Request === 'function' && module_or_path instanceof Request) || (typeof URL === 'function' && module_or_path instanceof URL)) {
        module_or_path = fetch(module_or_path);
    }

    const { instance, module } = await __wbg_load(await module_or_path, imports);

    return __wbg_finalize_init(instance, module);
}

export { initSync, __wbg_init as default };
