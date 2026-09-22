//! SpecFenceComputer — protocol note. The live pick is [`super::schedule::pick`]
//! over [`super::RunnableSet`]. This file must not host the Block-STM index walk.

#[cfg(test)]
mod tests {
    #[test]
    fn computer_source_never_calls_block_stm_pick() {
        let src = include_str!("computer.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        assert!(!code.contains("next_occ_task"));
        assert!(!code.contains("next_task"));
        assert!(!code.contains("next_sf_task"));
    }
}
