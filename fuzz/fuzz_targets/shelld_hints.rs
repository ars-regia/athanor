#![no_main]

libfuzzer_sys::fuzz_target!(|data: &[u8]| athanor_fuzz_entries::must(athanor_fuzz_entries::shelld::hints(data)));
