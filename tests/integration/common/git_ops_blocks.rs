//! Invariants
//! - The blocks in [`GitOpsBlocks`] all have non-empty lists of [`GitOp`]s

use super::GitOp;

/// List of dates, each of which has a non-empty list of [`GitOp`] operations
#[derive(Clone, Debug, Default)]
pub(super) struct GitOpsBlocks<'a> {
    blocks: Vec<(&'a str, Vec<GitOp<'a>>)>,
}
impl<'a> GitOpsBlocks<'a> {
    pub fn push_date(&mut self, date: &'a str, first_op: GitOp<'a>) {
        let Self { blocks } = self;
        // new entries start with 1 op
        blocks.push((date, vec![first_op]));
    }
    pub fn push_op_to_last_date(&mut self, op: GitOp<'a>) -> Result<(), ()> {
        let Some((_, last_ops_list)) = self.blocks.last_mut() else {
            return Err(());
        };
        // increase ops list length
        last_ops_list.push(op);
        Ok(())
    }
    pub fn iter(&self) -> impl Iterator<Item = (&'a str, &[GitOp<'a>])> {
        let GitOpsBlocks { blocks } = self;
        blocks.iter().map(|(date, ops)| (*date, &**ops))
    }
}
