use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::StroemGateway;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "(quote: CheckedQuote) => void")]
    pub type QuoteCallback;
    #[wasm_bindgen(typescript_type = "(update: SwapStatusUpdate) => void")]
    pub type SwapStatusCallback;
    #[wasm_bindgen(typescript_type = "(count: number) => void")]
    pub type PeerCountCallback;
}

#[wasm_bindgen]
impl StroemGateway {
    #[wasm_bindgen(js_name = onQuote)]
    pub fn on_quote(&self, callback: QuoteCallback) {
        let f: js_sys::Function = callback.unchecked_into();
        self.inner.callbacks.lock().quote.push(f);
    }

    #[wasm_bindgen(js_name = onSwapStatus)]
    pub fn on_swap_status(&self, callback: SwapStatusCallback) {
        let f: js_sys::Function = callback.unchecked_into();
        self.inner.callbacks.lock().swap_status.push(f);
    }

    #[wasm_bindgen(js_name = onPeerCount)]
    pub fn on_peer_count(&self, callback: PeerCountCallback) {
        let f: js_sys::Function = callback.unchecked_into();
        self.inner.callbacks.lock().peer_count.push(f);
    }
}
