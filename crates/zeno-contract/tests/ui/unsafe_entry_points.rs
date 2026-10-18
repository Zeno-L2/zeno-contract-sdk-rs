use zeno_contract::__private::{enter, finish};

fn main() {
    let _ = enter(0, 0);
    let ctx = unsafe { enter(0, 0) }.0;
    let _ = finish(&ctx, &());
}
