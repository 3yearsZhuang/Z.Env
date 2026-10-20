//! Node 稳定版本约束的保守子集；无法精确解释的语法返回未知。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct Version(pub [u64; 3]);
#[derive(Clone, Copy, Debug)]
struct Interval {
    low: Version,
    high: Option<Version>,
}
#[derive(Clone, Debug)]
pub(super) struct Constraint(Vec<Interval>);
fn number(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 1 && s.starts_with('0') || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = s.parse().ok()?;
    (n <= u32::MAX as u64).then_some(n)
}
fn parts(s: &str) -> Option<Vec<u64>> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let p: Option<Vec<_>> = s.split('.').map(number).collect();
    let p = p?;
    (1..=3).contains(&p.len()).then_some(p)
}
impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let p = parts(s)?;
        (p.len() == 3).then(|| Self([p[0], p[1], p[2]]))
    }
    fn successor(self) -> Option<Self> {
        let mut v = self.0;
        v[2] = v[2].checked_add(1)?;
        Some(Self(v))
    }
}
fn intersect(a: Interval, b: Interval) -> Option<Interval> {
    let low = a.low.max(b.low);
    let high = match (a.high, b.high) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    high.is_none_or(|h| low < h)
        .then_some(Interval { low, high })
}
impl Constraint {
    /// 合并重叠与相邻区间，避免重复的 OR 声明在连续求交时指数膨胀。
    fn normalized(mut intervals: Vec<Interval>) -> Self {
        intervals.retain(|i| i.high.is_none_or(|h| i.low < h));
        intervals.sort_by_key(|i| i.low);
        let mut merged: Vec<Interval> = Vec::new();
        for interval in intervals {
            if let Some(last) = merged.last_mut() {
                if last.high.is_none_or(|high| interval.low <= high) {
                    last.high = match (last.high, interval.high) {
                        (Some(a), Some(b)) => Some(a.max(b)),
                        _ => None,
                    };
                    continue;
                }
            }
            merged.push(interval);
        }
        Self(merged)
    }
    fn any() -> Self {
        Self(vec![Interval {
            low: Version([0, 0, 0]),
            high: None,
        }])
    }
    pub fn matches(&self, v: Version) -> bool {
        self.0
            .iter()
            .any(|i| v >= i.low && i.high.is_none_or(|h| v < h))
    }
    pub fn intersect(&self, other: &Self) -> Self {
        Self::normalized(
            self.0
                .iter()
                .flat_map(|a| other.0.iter().filter_map(move |b| intersect(*a, *b)))
                .collect(),
        )
    }
    pub fn empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn parse(text: &str) -> Option<Self> {
        if text.len() > 256
            || text.trim().is_empty()
            || text.contains([',', '\n', '\r', '-', '+', '{', '$'])
        {
            return None;
        }
        let mut union = vec![];
        let alternatives: Vec<_> = text.split("||").collect();
        if alternatives.len() > 8 {
            return None;
        }
        for alternative in alternatives {
            let mut current = Self::any();
            let mut count = 0;
            for token in alternative.split_whitespace() {
                count += 1;
                if count > 16 {
                    return None;
                }
                current = current.intersect(&Self(vec![token_interval(token)?]));
            }
            if count == 0 {
                return None;
            }
            union.extend(current.0);
        }
        Some(Self::normalized(union))
    }
}
fn token_interval(token: &str) -> Option<Interval> {
    let (operator, raw) = [">=", "<=", ">", "<", "=", "^", "~"]
        .into_iter()
        .find_map(|op| token.strip_prefix(op).map(|raw| (op, raw)))
        .unwrap_or(("", token));
    if matches!(raw, "*" | "x" | "X") {
        return (operator.is_empty() || operator == "=").then_some(Interval {
            low: Version([0, 0, 0]),
            high: None,
        });
    }
    let raw = raw.strip_prefix('v').unwrap_or(raw);
    let words: Vec<_> = raw.split('.').collect();
    if words.len() > 3 {
        return None;
    }
    let mut p = vec![];
    let mut wildcard = false;
    for word in words {
        if matches!(word, "*" | "x" | "X") {
            wildcard = true;
        } else {
            if wildcard {
                return None;
            }
            p.push(number(word)?);
        }
    }
    if p.is_empty() {
        return None;
    }
    if wildcard && !matches!(operator, "" | "=") {
        return None;
    }
    let low = Version([p[0], *p.get(1).unwrap_or(&0), *p.get(2).unwrap_or(&0)]);
    let upper_partial = || {
        let mut v = low.0;
        let index = p.len() - 1;
        v[index] = v[index].checked_add(1)?;
        for item in v.iter_mut().skip(index + 1) {
            *item = 0;
        }
        Some(Version(v))
    };
    let interval = match operator {
        "" | "=" => Interval {
            low,
            high: Some(if p.len() == 3 {
                low.successor()?
            } else {
                upper_partial()?
            }),
        },
        ">=" => Interval { low, high: None },
        ">" => Interval {
            low: if p.len() == 3 {
                low.successor()?
            } else {
                upper_partial()?
            },
            high: None,
        },
        "<" => Interval {
            low: Version([0, 0, 0]),
            high: Some(low),
        },
        "<=" => Interval {
            low: Version([0, 0, 0]),
            high: Some(if p.len() == 3 {
                low.successor()?
            } else {
                upper_partial()?
            }),
        },
        "~" => {
            let mut high = low.0;
            let index = if p.len() == 1 { 0 } else { 1 };
            high[index] = high[index].checked_add(1)?;
            for v in high.iter_mut().skip(index + 1) {
                *v = 0;
            }
            Interval {
                low,
                high: Some(Version(high)),
            }
        }
        "^" => {
            let index = if low.0[0] > 0 || p.len() == 1 {
                0
            } else if low.0[1] > 0 || p.len() == 2 {
                1
            } else {
                2
            };
            let mut high = low.0;
            high[index] = high[index].checked_add(1)?;
            for v in high.iter_mut().skip(index + 1) {
                *v = 0;
            }
            Interval {
                low,
                high: Some(Version(high)),
            }
        }
        _ => return None,
    };
    Some(interval)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supported_subset_matches_independent_npm_semver_oracle() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/project-environment/node-ranges-v1.json"
        ))
        .unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let range = case["range"].as_str().unwrap();
            let parsed = Constraint::parse(range).unwrap();
            for result in case["results"].as_array().unwrap() {
                let raw = result["version"].as_str().unwrap();
                assert_eq!(
                    parsed.matches(Version::parse(raw).unwrap()),
                    result["matches"].as_bool().unwrap(),
                    "{range} / {raw}"
                );
            }
        }
    }
    #[test]
    fn stable_npm_ranges_keep_major_minor_comparators_and_zero_caret_boundaries() {
        for (range, yes, no) in [
            ("22", "22.9.0", "20.1.0"),
            ("22.1.x", "22.1.9", "22.2.0"),
            ("^22.1.0", "22.9.0", "23.0.0"),
            ("~22.1.0", "22.1.8", "22.2.0"),
            ("^0.2.3", "0.2.9", "0.3.0"),
            ("^0.0.3", "0.0.3", "0.0.4"),
            (">=20 <23", "22.8.0", "23.0.0"),
            ("20 || >=22.1.0 <23", "22.8.0", "21.0.0"),
            (">22", "23.0.0", "22.9.9"),
            ("<=22.1", "22.1.9", "22.2.0"),
        ] {
            let constraint = Constraint::parse(range).unwrap();
            assert!(constraint.matches(Version::parse(yes).unwrap()), "{range}");
            assert!(!constraint.matches(Version::parse(no).unwrap()), "{range}");
        }
    }
    #[test]
    fn unsupported_aliases_prereleases_templates_and_partial_syntax_never_become_any_version() {
        for text in [
            "",
            "lts/*",
            "latest",
            "22.1.0-beta",
            "22.0.0 - 23.0.0",
            ">= 22",
            "22.*.1",
            "^22.x",
            "01",
            "node@22",
            "{{exec}}",
            "https://nodejs.org",
            "22 ||",
            "22,23",
        ] {
            assert!(Constraint::parse(text).is_none(), "{text}");
        }
        assert!(Constraint::parse(">=22 <20").is_some());
    }
    #[test]
    fn contradictory_ranges_are_detected_without_relying_on_installed_candidates() {
        assert!(Constraint::parse("<0").unwrap().empty());
        let range = Constraint::parse("20 || 22 || 22 || 22 || 22 || 22 || 22 || 24").unwrap();
        let mut combined = range.clone();
        for _ in 0..1000 {
            combined = combined.intersect(&range);
        }
        assert_eq!(combined.0.len(), 3);
        assert!(Constraint::parse("22")
            .unwrap()
            .intersect(&Constraint::parse("<21").unwrap())
            .empty());
        assert!(!Constraint::parse("^22.1.0")
            .unwrap()
            .intersect(&Constraint::parse(">=22 <23").unwrap())
            .empty());
    }
}
