#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum MetricMode {
    Code,
    Total,
    Unique,
    Shared,
}

impl MetricMode {
    pub(crate) const ALL: [Self; 4] = [Self::Code, Self::Total, Self::Unique, Self::Shared];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Code => "code loc",
            Self::Total => "total reach",
            Self::Unique => "unique loc",
            Self::Shared => "shared loc",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum FilterMode {
    All,
    Runtime,
    Build,
    Unique,
    Shared,
}

impl FilterMode {
    pub(crate) const ALL: [Self; 5] = [Self::All, Self::Runtime, Self::Build, Self::Unique, Self::Shared];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Runtime => "runtime",
            Self::Build => "build",
            Self::Unique => "unique",
            Self::Shared => "shared",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ColorMode {
    Layer,
    SourceKind,
    Ecosystem,
    Health,
}

impl ColorMode {
    pub(crate) const ALL: [Self; 4] = [Self::Layer, Self::SourceKind, Self::Ecosystem, Self::Health];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Layer => "layer",
            Self::SourceKind => "kind",
            Self::Ecosystem => "ecosystem",
            Self::Health => "health",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum HierarchyMode {
    SourceKind,
    Ecosystem,
    Layer,
}

impl HierarchyMode {
    pub(crate) const ALL: [Self; 3] = [Self::SourceKind, Self::Ecosystem, Self::Layer];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::SourceKind => "kind",
            Self::Ecosystem => "ecosystem",
            Self::Layer => "layer",
        }
    }

    pub(crate) fn view_name(self) -> &'static str {
        match self {
            Self::SourceKind => "source-kind",
            Self::Ecosystem => "ecosystem",
            Self::Layer => "layer",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum CompletenessMode {
    Counted,
    All,
    Generated,
    Missing,
}

impl CompletenessMode {
    pub(crate) const ALL: [Self; 4] = [Self::Counted, Self::All, Self::Generated, Self::Missing];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Counted => "counted",
            Self::All => "all health",
            Self::Generated => "generated",
            Self::Missing => "missing",
        }
    }
}

pub(crate) fn cycle<T: Copy + Eq>(current: T, values: &[T]) -> T {
    let index = values.iter().position(|value| *value == current).unwrap_or(0);
    values[(index + 1) % values.len()]
}
