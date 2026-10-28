//! Outputs a modified fixture by replacing all file names with (one-to-one matching) placeholder
//! names

use self::imp::AnonymizeFixture;
use eyre::Context as _;

fn main() -> eyre::Result<()> {
    for out_result in AnonymizeFixture::new(std::io::stdin().lines()) {
        let out_line = out_result.context("failed to read stdin")?;

        println!("{out_line}");
    }
    Ok(())
}

mod imp {
    use std::collections::HashMap;

    #[derive(Default)]
    pub struct PathAnonymizer {
        path_map: HashMap<String, String>,
        dir_counts: Vec<usize>,
        file_count: usize,
    }
    impl PathAnonymizer {
        pub fn get_or_insert(&mut self, path: &str) -> String {
            if let Some(value) = self.path_map.get(path) {
                return value.clone();
            }
            let value = self.generate_new(path);
            self.path_map.insert(path.to_string(), value.clone());
            value
        }
        fn generate_new(&mut self, path: &str) -> String {
            use std::fmt::Write as _;

            let path = std::path::PathBuf::from(path);
            let mut out = String::new();

            let mut components = path.iter().enumerate().peekable();
            while let Some((i, _component)) = components.next() {
                if components.peek().is_some() {
                    // dir
                    if self.dir_counts.len() <= i {
                        self.dir_counts.push(0);
                    }
                    let count = &mut self.dir_counts[i];
                    *count = count.checked_add(1).expect("too many folders");
                    write!(&mut out, "dir-{count}/")
                } else {
                    //file
                    let count = &mut self.file_count;
                    *count = count.checked_add(1).expect("too many files");
                    write!(&mut out, "file-{count}")
                }
                .expect("fmt infallible");
            }
            out
        }
    }

    pub struct AnonymizeFixture<I> {
        iter: Option<I>,
        logic: PathAnonymizer,
    }
    impl<I, E> AnonymizeFixture<I>
    where
        I: Iterator<Item = Result<String, E>>,
    {
        pub fn new(lines: impl IntoIterator<IntoIter = I, Item = I::Item>) -> Self {
            Self {
                iter: Some(lines.into_iter()),
                logic: PathAnonymizer::default(),
            }
        }
    }
    impl<I, E> Iterator for AnonymizeFixture<I>
    where
        I: Iterator<Item = Result<String, E>>,
    {
        type Item = Result<String, E>;
        fn next(&mut self) -> Option<Self::Item> {
            let Self { iter, logic } = self;
            let iter = iter.as_mut()?;

            let value = match iter.next()? {
                Ok(value) => value,
                Err(e) => {
                    self.iter.take();
                    return Some(Err(e));
                }
            };

            let output = match (value.get(0..1), value.get(1..)) {
                (Some(prefix @ ("+" | "-")), Some(rest)) => {
                    let placeholder = logic.get_or_insert(rest);
                    format!("{prefix}{placeholder}")
                }
                _ => value,
            };
            Some(Ok(output))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::imp::AnonymizeFixture;

    fn uut(input: &str) -> String {
        use std::fmt::Write as _;

        let out_lines: Result<Vec<_>, !> =
            AnonymizeFixture::new(input.lines().map(|s| Ok(s.to_string()))).collect();
        let Ok(out_lines) = out_lines;

        let mut out = String::new();
        for line in out_lines {
            writeln!(&mut out, "{line}").expect("infallible fmt");
        }
        out
    }
    macro_rules! test_uut {
        ($($input:ident : $expect:expr),+ $(,)?) => {
            $(
                let input = $input;
                assert_eq!(uut(input), $expect, "failed for input: {input:?}");
            )+
        };
    }

    #[test]
    fn empty() {
        let empty = "";
        test_uut!(empty: "");
    }

    #[test]
    fn one() {
        let plus = "
2026-01-01:
+entry.txt
";
        test_uut!(
            plus: "
2026-01-01:
+file-1
",
        );

        let minus = "
2026-01-01:
-entry.txt
";
        test_uut!(
            minus: "
2026-01-01:
-file-1
",
        );
    }

    #[test]
    fn many() {
        let input = "
2026-12-22:
+.gitignore

2026-12-22:
+.gitattributes

2026-12-22:
+folder/file.txt
-.gitignore
";
        test_uut!(input: "
2026-12-22:
+file-1

2026-12-22:
+file-2

2026-12-22:
+dir-1/file-3
-file-1
",
        );
    }
}
