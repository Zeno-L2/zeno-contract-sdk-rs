use zeno_contract::{Codec, Key, Outputs, Writer};

struct Mine;

impl<'a> Codec<'a> for Mine {
    const ZENO_TYPE: &'static str = "u8";
    fn encode(&self, _: &mut Writer<'_>) {}
    fn decode(_: &mut zeno_contract::__private::Reader<'a>) -> Option<Self> {
        Some(Mine)
    }
}

impl Key for Mine {
    const FIELDS: usize = 1;
    const ENCODED_LEN: usize = 1;
    const TYPES: &'static [&'static str] = &["u8"];
    fn encode(&self, _: &mut Writer<'_>) {}
}

impl Outputs for Mine {
    const COUNT: usize = 1;
    const TYPES: &'static [&'static str] = &["u8"];
    fn encode(&self, _: &mut Writer<'_>) {}
    fn decode(_: &mut zeno_contract::__private::Reader<'_>) -> Option<Self> {
        None
    }
}

fn main() {}
