//! TSPS-only candidate: the observed VConfig+192 bit 0 bypasses the H.264 output hold gate.
//! No binary or system-library patch. Refuse a different vendor implementation.
const SORT_GATE_OFFSET:usize=0x21928;
const SORT_GATE:[u8;20]=[0xe0,0x17,0x40,0xf9,0x00,0x28,0x41,0xb9,0x00,0x00,0x00,0x12,0x1f,0x00,0x00,0x71,0x80,0x46,0x00,0x54];
const CONFIG_COPY_OFFSET:usize=0x17ab4;
const CONFIG_COPY:[u8;24]=[0xe0,0x27,0x40,0xf9,0x00,0xa0,0x01,0x91,0x02,0x1b,0x80,0xd2,0xe1,0x13,0x40,0xf9,0x47,0xb2,0xff,0x97,0xe0,0x27,0x40,0xf9];
pub(crate) fn supports_vendor(blob:&[u8])->bool {
 blob.get(SORT_GATE_OFFSET..SORT_GATE_OFFSET+SORT_GATE.len())==Some(SORT_GATE.as_slice())
 && blob.get(CONFIG_COPY_OFFSET..CONFIG_COPY_OFFSET+CONFIG_COPY.len())==Some(CONFIG_COPY.as_slice())
}
pub(crate) fn select(mode:i32,safe:bool,vendor:bool)->Result<bool,&'static str> {
 match mode {
  0=>Ok(false),
  -1=>Ok(safe && vendor),
  1 if safe && vendor=>Ok(true),
  1=>Err("cedar: forced low-delay requires verified vendor and progressive POC type 2 without B slices"),
  _=>Err("cedar: invalid low-delay selection"),
 }
}
pub(crate) fn safe_stream(poc_type:u8,progressive:bool,has_b:bool)->bool {
 poc_type==2 && progressive && !has_b
}
#[cfg(test)]
mod tests {
 use super::*;
 #[test] fn binary_pins_require_both_exact_instruction_windows() {
  let mut b=vec![0u8;SORT_GATE_OFFSET+SORT_GATE.len()];
  b[SORT_GATE_OFFSET..SORT_GATE_OFFSET+SORT_GATE.len()].copy_from_slice(&SORT_GATE);
  assert!(!supports_vendor(&b));
  b[CONFIG_COPY_OFFSET..CONFIG_COPY_OFFSET+CONFIG_COPY.len()].copy_from_slice(&CONFIG_COPY);
  assert!(supports_vendor(&b));
  b[SORT_GATE_OFFSET+4]^=1;assert!(!supports_vendor(&b));
  assert!(!supports_vendor(&[]));
 }
 #[test] fn auto_preserves_baseline_when_either_guard_is_absent() {
  assert_eq!(select(-1,true,true),Ok(true));
  assert_eq!(select(-1,false,true),Ok(false));
  assert_eq!(select(-1,true,false),Ok(false));
  assert_eq!(select(0,true,true),Ok(false));
  assert!(select(1,false,true).is_err());
  assert!(select(1,true,false).is_err());
  assert_eq!(select(1,true,true),Ok(true));
 }
 #[test] fn stream_gate_refuses_reordering_and_interlacing() {
  assert!(safe_stream(2,true,false));
  for (p,i,b) in [(0,true,false),(1,true,false),(2,false,false),(2,true,true)] { assert!(!safe_stream(p,i,b)); }
 }
}
