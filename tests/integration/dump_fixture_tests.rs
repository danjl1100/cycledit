use crate::common::{BlockVisitor, BlocksIter, BlocksVisitor, GitOp, PathAndParent, TestHarness};

/// Multi-commit fixture for walk-metrics baseline.
///
/// `trash/` is created across two early commits then entirely deleted, leaving only
/// `keep/` files at HEAD.  The optimizations (HEAD-only candidate set, early exit,
/// tree-pruning) skip the deleted subtree and short-circuit the walk, cutting
/// `find_object` calls roughly in half compared to the unoptimized algorithm.
const METRICS_FIXTURE: &str = "
2024-01-01:
+trash/a.txt
+trash/b.txt
+trash/c.txt
+trash/d.txt
+trash/e.txt

2024-02-01:
+trash/f.txt
-trash/a.txt
-trash/b.txt

2024-03-01:
-trash/c.txt
-trash/d.txt
-trash/e.txt
-trash/f.txt

2024-04-01:
+keep/file1.txt
+keep/file2.txt
+keep/file3.txt

2024-05-01:
+keep/file4.txt
+keep/file5.txt
";

#[test]
fn metrics_baseline() -> eyre::Result<()> {
    let output = TestHarness::new()?
        .init_git(METRICS_FIXTURE)?
        .with_metrics()
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;
    insta::assert_snapshot!(output.stderr, @"metrics: find_object_calls=11, visited_dirs=3, visited_files=11");
    Ok(())
}

#[test]
fn round_trip_subdirectory() -> eyre::Result<()> {
    let fixture = "
2024-06-01:
+README.md
+src/main.rs
+src/util/helper.rs
";
    let harness = TestHarness::new()?.init_git(fixture)?;
    let dumped = harness.dump_fixture()?;
    assert_eq!(dumped, fixture);

    let list_output = TestHarness::new()?
        .init_git(&dumped)?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;
    insta::assert_snapshot!(
        list_output.stdout,
        @r"
    2024-06-01 README.md
    2024-06-01 src/main.rs
    2024-06-01 src/util/helper.rs
    "
    );
    Ok(())
}

#[test]
fn round_trip_single_add() -> eyre::Result<()> {
    let fixture = "
2024-01-15:
+foo.txt
";
    let harness = TestHarness::new()?.init_git(fixture)?;
    let dumped = harness.dump_fixture()?;
    assert_eq!(dumped, fixture);

    let list_output = TestHarness::new()?
        .init_git(&dumped)?
        .run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;
    insta::assert_snapshot!(
        list_output.stdout,
        @"2024-01-15 foo.txt"
    );
    Ok(())
}

#[test]
fn round_trip_add_and_remove() -> eyre::Result<()> {
    let fixture = "
2024-01-15:
+foo.txt

2024-03-20:
+bar.txt
-foo.txt
";
    let harness = TestHarness::new()?.init_git(fixture)?;
    let dumped = harness.dump_fixture()?;
    assert_eq!(dumped, fixture);

    let harness2 = TestHarness::new()?.init_git(&dumped)?;
    assert_eq!(harness2.commit_count()?, 2);
    let list_output = harness2.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;
    insta::assert_snapshot!(
        list_output.stdout,
        @"2024-03-20 bar.txt"
    );
    Ok(())
}

#[test]
fn round_trip_multiple_files() -> eyre::Result<()> {
    let fixture = "
2024-01-15:
+aaa.txt
+mmm.txt
+zzz.txt

2024-03-20:
+aaa2.txt
+bbb.txt
";
    let harness = TestHarness::new()?.init_git(fixture)?;
    let dumped = harness.dump_fixture()?;
    assert_eq!(dumped, fixture);

    let harness2 = TestHarness::new()?.init_git(&dumped)?;
    assert_eq!(harness2.commit_count()?, 2);
    let list_output = harness2.run_cli("2026-01-01T00:00:00+00:00[UTC]", &["list"])?;
    insta::assert_snapshot!(
        list_output.stdout,
        @r"
    2024-01-15 aaa.txt
    2024-03-20 aaa2.txt
    2024-03-20 bbb.txt
    2024-01-15 mmm.txt
    2024-01-15 zzz.txt
    "
    );
    Ok(())
}

struct DatedBlocks<'a> {
    blocks: &'a [(&'a str, HighDepthTree<'a>)],
}
impl BlocksIter for &DatedBlocks<'_> {
    fn visit_all<T: BlocksVisitor>(self, visitor: T) -> Result<(), T::Error> {
        let DatedBlocks { blocks } = self;
        let mut visitor = Some(visitor);
        for block in *blocks {
            let (
                date,
                HighDepthTree {
                    trees,
                    files_in_each_dir,
                    regular_paths,
                },
            ) = block;

            let mut block_visitor = visitor
                .take()
                .expect("maintain visitor")
                .start_block(date)?;

            for tree in trees {
                use std::fmt::Write as _;

                const TREE_SEPARATOR: char = '/';
                let full_tree = {
                    let mut s = String::new();
                    for p in tree {
                        if !s.is_empty() {
                            write!(&mut s, "{TREE_SEPARATOR}").expect("infallible");
                        }
                        write!(&mut s, "{p}").expect("infallible");
                    }
                    s
                };

                let mut tree = &full_tree[..];
                while let Some((next_tree, _last)) = tree.rsplit_once(TREE_SEPARATOR) {
                    // write all the files in this dir
                    for &(op, file, count) in files_in_each_dir {
                        for i in 0..=count {
                            let path = format!("{tree}/{file}{i}");
                            let op = op.with_path(&path).expect("contains name");
                            block_visitor.visit(op)?;
                        }
                    }
                    // visit the parent
                    tree = next_tree;
                }
            }
            for &(op, regular_path) in regular_paths {
                #[expect(clippy::panic)]
                let Some(op) = op.with_path(regular_path) else {
                    panic!("invalid regular_path, must have filename: {regular_path}")
                };
                block_visitor.visit(op)?;
            }

            visitor.replace(block_visitor.end()?);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
enum Op {
    Add,
    Remove,
}
impl Op {
    fn with_path(self, path: &str) -> Option<GitOp<'_>> {
        match self {
            Self::Add => PathAndParent::new(path).map(|path| GitOp::Add { parsed: path }),
            Self::Remove => Some(GitOp::Remove { path }),
        }
    }
}
impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let op = match self {
            Op::Add => '+',
            Op::Remove => '-',
        };
        write!(f, "{op}")
    }
}
#[derive(Clone, Debug)]
struct HighDepthTree<'a> {
    trees: Vec<Vec<&'a str>>,
    files_in_each_dir: Vec<(Op, &'a str, u32)>,
    regular_paths: Vec<(Op, &'a str)>,
}
// TODO remove if unused (mostly a proof-of-concept for the visitor traits)
impl std::fmt::Display for DatedBlocks<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        struct Visitor<'a, 'b>(&'a mut std::fmt::Formatter<'b>);
        impl BlocksVisitor for Visitor<'_, '_> {
            type Error = std::fmt::Error;
            type BlockVisitor<'a>
                = Self
            where
                Self: 'a;

            fn start_block(mut self, date: &str) -> Result<Self, Self::Error> {
                let Self(f) = &mut self;
                writeln!(f, "{date}:")?;

                Ok(self)
            }
        }
        impl BlockVisitor<'_, Self> for Visitor<'_, '_> {
            type Error = std::fmt::Error;
            fn visit(&mut self, op: GitOp<'_>) -> Result<(), Self::Error> {
                let Self(f) = self;
                let (op, path) = match op {
                    GitOp::Add { parsed } => {
                        let path = parsed.get_path();
                        ('+', path)
                    }
                    GitOp::Remove { path } => ('-', path),
                };
                writeln!(f, "{op}{path}")
            }

            fn end(mut self) -> Result<Self, Self::Error> {
                let Self(f) = &mut self;
                writeln!(f)?;

                Ok(self)
            }
        }

        self.visit_all(Visitor(f))
    }
}

struct PrefixedList(Vec<String>);
impl PrefixedList {
    fn new(prefix: &str, iter: impl Iterator<Item: std::fmt::Display>) -> Self {
        let list = iter.map(|elem| format!("{prefix}{elem}")).collect();
        Self(list)
    }
    fn collect_refs(&self) -> Vec<&str> {
        let Self(list) = self;
        list.iter().map(|s| &**s).collect()
    }
}

#[test]
fn metrics_high_depth() -> eyre::Result<()> {
    let numbers = PrefixedList::new("folder_", 0..50);
    let numbers = numbers.collect_refs();
    let blocks = DatedBlocks {
        blocks: &[(
            "2025-06-24",
            HighDepthTree {
                trees: vec![
                    //
                    numbers,
                    vec!["other", "tree"],
                ],
                files_in_each_dir: vec![(Op::Add, "f", 1)],
                regular_paths: vec![
                    (Op::Add, "regular1.txt"),
                    (Op::Add, "zzz/another_regular.txt"),
                ],
            },
        )],
    };

    let harness = TestHarness::new()?
        .init_git_from_blocks(&blocks)?
        .with_metrics();

    let output = harness.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["list", "regular1.txt", "zzz/*"],
    )?;
    insta::assert_snapshot!(output.stderr, @"metrics: find_object_calls=55, visited_dirs=53, visited_files=102");
    insta::assert_snapshot!(output.stdout, @r"
    2025-06-24 regular1.txt
    2025-06-24 zzz/another_regular.txt
    ");
    let output_stdout_1 = output.stdout;

    let output = harness.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["list", "--exclude", "folder_*", "--exclude", "other*"],
    )?;
    insta::assert_snapshot!(output.stderr, @"metrics: find_object_calls=3, visited_dirs=1, visited_files=2");
    assert_eq!(output_stdout_1, output.stdout);

    Ok(())
}

#[test]
fn metrics_wide_breadth() -> eyre::Result<()> {
    let letters = PrefixedList::new("folder_", 'a'..='g');
    let letters = letters.collect_refs();

    let numbers = PrefixedList::new("folder_", 1..=5);
    let numbers = numbers.collect_refs();

    let mixed: Vec<_> = (0..40).map(|n| format!("mixed_{n}")).collect();

    let large_forest_of_trees: Vec<_> = letters
        .iter()
        .copied()
        .chain(numbers.iter().copied())
        .chain(mixed.iter().map(|s| &**s))
        .map(|first| vec![first, "tree"])
        .collect();

    let blocks = DatedBlocks {
        blocks: &[
            (
                "2025-01-01",
                HighDepthTree {
                    trees: large_forest_of_trees,
                    files_in_each_dir: vec![(Op::Add, "f", 2)],
                    regular_paths: vec![
                        (Op::Add, "distraction.txt"),
                        (Op::Add, "another/distraction.txt"),
                    ],
                },
            ),
            (
                "2025-01-02",
                HighDepthTree {
                    trees: vec![letters, numbers],
                    files_in_each_dir: vec![(Op::Add, "f", 1)],
                    regular_paths: vec![
                        (Op::Add, "mix_valid_24/plausible_tree/regular1.txt"),
                        (Op::Add, "zzz/another_regular.txt"),
                    ],
                },
            ),
        ],
    };

    let harness = TestHarness::new()?
        .init_git_from_blocks(&blocks)?
        .with_metrics();

    let output = harness.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &["list", "regular1.txt", "zzz/*"],
    )?;
    insta::assert_snapshot!(output.stderr, @"metrics: find_object_calls=122, visited_dirs=118, visited_files=181");
    insta::assert_snapshot!(output.stdout, @r"
    2025-01-02 mix_valid_24/plausible_tree/regular1.txt
    2025-01-02 zzz/another_regular.txt
    ");
    let output_stdout_1 = output.stdout;

    let output = harness.run_cli(
        "2026-01-01T00:00:00+00:00[UTC]",
        &[
            "list",
            "--exclude",
            "folder_*",
            "--exclude",
            "mixed_*",
            "--exclude",
            "distraction.txt",
        ],
    )?;
    insta::assert_snapshot!(output.stderr, @"metrics: find_object_calls=8, visited_dirs=4, visited_files=5");
    assert_eq!(output_stdout_1, output.stdout);

    Ok(())
}

#[test]
fn metrics_long_commit_history() -> eyre::Result<()> {
    let letters = PrefixedList::new("folder_", 'a'..='g');
    let letters = letters.collect_refs();

    let trees_for_op = |op| HighDepthTree {
        trees: vec![letters.clone()],
        files_in_each_dir: vec![(op, "toggle_file", 1)],
        regular_paths: vec![],
    };

    let many_final_files = PrefixedList::new("final_", 0..20);
    let many_final_files = many_final_files.collect_refs();

    let final_files: Vec<_> = [
        (Op::Add, "recent_file_1.txt"),
        (Op::Add, "recent_file_2.txt"),
        (Op::Remove, "ancient_file_will_be_deleted.txt"),
    ]
    .into_iter()
    .chain(many_final_files.into_iter().map(|s| (Op::Add, s)))
    .collect();

    let blocks = DatedBlocks {
        blocks: &[
            (
                "1970-01-29",
                HighDepthTree {
                    trees: vec![],
                    files_in_each_dir: vec![],
                    regular_paths: vec![
                        //
                        (Op::Add, "ancient_file.txt"),
                        (Op::Add, "ancient_file_will_be_deleted.txt"),
                    ],
                },
            ),
            ("1980-01-01", trees_for_op(Op::Add)),
            ("1980-01-02", trees_for_op(Op::Remove)),
            ("1980-01-03", trees_for_op(Op::Add)),
            ("1980-01-04", trees_for_op(Op::Remove)),
            ("1980-01-05", trees_for_op(Op::Add)),
            ("1980-01-06", trees_for_op(Op::Remove)),
            ("1980-01-07", trees_for_op(Op::Add)),
            ("1980-01-08", trees_for_op(Op::Remove)),
            ("1980-01-09", trees_for_op(Op::Add)),
            ("1980-01-10", trees_for_op(Op::Remove)),
            ("1980-01-11", trees_for_op(Op::Add)),
            ("1980-01-12", trees_for_op(Op::Remove)),
            ("1980-01-13", trees_for_op(Op::Add)),
            ("1980-01-14", trees_for_op(Op::Remove)),
            ("1980-01-15", trees_for_op(Op::Add)),
            ("1980-01-16", trees_for_op(Op::Remove)),
            ("1980-01-17", trees_for_op(Op::Add)),
            ("1980-01-18", trees_for_op(Op::Remove)),
            ("1980-01-19", trees_for_op(Op::Add)),
            ("1980-01-20", trees_for_op(Op::Remove)),
            (
                "1980-02-01",
                HighDepthTree {
                    trees: vec![],
                    files_in_each_dir: vec![],
                    regular_paths: final_files,
                },
            ),
        ],
    };

    let harness = TestHarness::new()?
        .init_git_from_blocks(&blocks)?
        .with_metrics();

    let output = harness.run_cli(
        "2000-01-01T00:00:00[UTC]",
        &["list", "--exclude", "final_*"],
    )?;
    insta::assert_snapshot!(output.stderr, @"metrics: find_object_calls=86, visited_dirs=0, visited_files=107");
    insta::assert_snapshot!(output.stdout, @r"
    1970-01-29 ancient_file.txt
    1980-02-01 recent_file_1.txt
    1980-02-01 recent_file_2.txt
    ");

    let output = harness.run_cli(
        "2000-01-01T00:00:00[UTC]",
        &[
            "list",
            "--exclude",
            "ancient_file.txt",
            "--exclude",
            "final_*",
        ],
    )?;
    insta::assert_snapshot!(output.stderr, @"metrics: find_object_calls=4, visited_dirs=0, visited_files=25");
    insta::assert_snapshot!(output.stdout, @r"
    1980-02-01 recent_file_1.txt
    1980-02-01 recent_file_2.txt
    ");
    let output_stdout_2 = output.stdout;

    let output = harness.run_cli("2000-01-01T00:00:00[UTC]", &["list", "recent_file_*.txt"])?;
    insta::assert_snapshot!(output.stderr, @"metrics: find_object_calls=4, visited_dirs=0, visited_files=25");
    assert_eq!(output_stdout_2, output.stdout);

    Ok(())
}
