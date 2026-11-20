//! ONNX Runtime set-up shared by every model speak runs (Kokoro, the English G2P network).

use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use ort::environment::Environment;
use ort::session::Session;
use ort::session::builder::{AutoDevicePolicy, SessionBuilder};

/// The process's ONNX Runtime environment. ort allows only one per process, so every session is
/// built on this one, and it lives until the process exits.
pub fn environment() -> Result<&'static Environment> {
    static ENV: OnceLock<Result<Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| ort::init().with_name("speak").build().map_err(|e| e.to_string())).as_ref().map_err(|e| anyhow!("{e}"))
}

/// A session builder on the CPU provider, with `threads` intra-op threads (0: ONNX Runtime's
/// default). ort's default device policy registers CoreML on macOS: seconds of extra load time and
/// numbers that drift from the references. `SPEAK_EP=auto` brings it back.
pub fn cpu_session(threads: usize) -> Result<SessionBuilder> {
    let policy = match std::env::var("SPEAK_EP").as_deref() {
        Ok("auto") => AutoDevicePolicy::MaxEfficiency,
        _ => AutoDevicePolicy::PreferCPU,
    };
    let builder = Session::builder(environment()?)?.with_auto_device(policy).map_err(|e| anyhow!("{e}"))?;
    Ok(if threads > 0 { builder.with_intra_threads(threads).map_err(|e| anyhow!("{e}"))? } else { builder })
}

/// Add the intermediate tensor `name`, of ONNX element type `elem_type`, to the graph outputs of
/// `model` (an ONNX ModelProto), so a session returns it next to the real outputs. False, and
/// `model` untouched, if no node produces `name`. Edits the protobuf wire format in place: the
/// graph's length prefix is rewritten and the output appended after it, so a model read with a
/// little spare capacity is never copied.
pub fn add_output(model: &mut Vec<u8>, name: &str, elem_type: u64) -> bool {
    let Some((span, graph)) = fields_with_spans(model).find(|f| f.0 == 7).and_then(|(_, w, span)| Some((span, w.bytes()?))) else {
        return false;
    };
    let g = &model[graph.clone()];
    let produced = fields(g).filter(|f| f.0 == 1).filter_map(|f| f.1.bytes()).any(|n| {
        let node = &g[n];
        fields(node).any(|f| f.0 == 2 && f.1.str(node) == Some(name))
    });
    if !produced {
        return false;
    }
    // ValueInfoProto { name: 1, type: 2 { tensor_type: 1 { elem_type: 1 } } }
    let tensor = [&[1 << 3][..], &varint(elem_type)].concat();
    let value_info = [field(1, name.as_bytes()), field(2, &field(1, &tensor))].concat();
    let output = field(12, &value_info);
    let key_len = varint(7 << 3 | 2).len();
    model.splice(graph.end..graph.end, output.iter().copied());
    model.splice(span.start + key_len..graph.start, varint((graph.len() + output.len()) as u64));
    true
}

#[derive(Clone, Debug, PartialEq)]
enum Wire {
    Varint(u64),
    Bytes(std::ops::Range<usize>),
    Fixed,
}

impl Wire {
    fn bytes(&self) -> Option<std::ops::Range<usize>> {
        match self {
            Wire::Bytes(r) => Some(r.clone()),
            _ => None,
        }
    }
    fn str<'a>(&self, buf: &'a [u8]) -> Option<&'a str> {
        std::str::from_utf8(&buf[self.bytes()?]).ok()
    }
}

fn read_varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*i)?;
        *i += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return Some(v);
        }
    }
    None
}

fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// A length-delimited field.
fn field(number: u64, payload: &[u8]) -> Vec<u8> {
    [varint(number << 3 | 2), varint(payload.len() as u64), payload.to_vec()].concat()
}

/// The top-level fields of a protobuf message: (field number, value, the field's whole byte span).
/// Stops at the first malformed field.
fn fields_with_spans(b: &[u8]) -> impl Iterator<Item = (u64, Wire, std::ops::Range<usize>)> + '_ {
    let mut i = 0;
    std::iter::from_fn(move || {
        let start = i;
        let key = read_varint(b, &mut i)?;
        let wire = match key & 7 {
            0 => Wire::Varint(read_varint(b, &mut i)?),
            1 | 5 => {
                i += if key & 7 == 1 { 8 } else { 4 };
                Wire::Fixed
            }
            2 => {
                let len = usize::try_from(read_varint(b, &mut i)?).ok()?;
                let r = i..i.checked_add(len)?;
                i = r.end;
                Wire::Bytes(r)
            }
            _ => return None,
        };
        (i <= b.len()).then_some((key >> 3, wire, start..i))
    })
}

fn fields(b: &[u8]) -> impl Iterator<Item = (u64, Wire)> + '_ {
    fields_with_spans(b).map(|(n, w, _)| (n, w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_an_intermediate_output() {
        // ModelProto { ir_version: 8, graph: { node: [{ output: "dur" }, { output: "audio" }], output: [{ name: "audio" }] } }
        let node = |out: &str| field(1, &field(2, out.as_bytes()));
        let graph = [node("dur"), node("audio"), field(12, &field(1, b"audio"))].concat();
        let model = [vec![1 << 3, 8], field(7, &graph), field(8, b"opset")].concat();

        let mut patched = model.clone();
        assert!(add_output(&mut patched, "dur", 7));
        let top: Vec<(u64, Wire)> = fields(&patched).collect();
        assert_eq!(top.iter().map(|f| f.0).collect::<Vec<_>>(), [1, 7, 8]);
        assert_eq!(top[0].1, Wire::Varint(8));
        let g = &patched[top[1].1.bytes().unwrap()];
        let outputs: Vec<&str> = fields(g)
            .filter(|f| f.0 == 12)
            .map(|f| {
                let info = &g[f.1.bytes().unwrap()];
                fields(info).next().unwrap().1.str(info).unwrap()
            })
            .collect();
        assert_eq!(outputs, ["audio", "dur"]);
        // The graph's length prefix grows from one byte to two here.
        let mut long = [vec![1 << 3, 8], field(7, &[graph.clone(), field(13, &[b'x'; 100])].concat())].concat();
        assert!(add_output(&mut long, "dur", 7));
        assert_eq!(fields(&long).count(), 2);
        let mut unchanged = model.clone();
        assert!(!add_output(&mut unchanged, "nope", 7));
        assert_eq!(unchanged, model);
        assert!(!add_output(&mut vec![0xff], "dur", 7));
    }
}
