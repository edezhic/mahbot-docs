//! One word per variant: the serialised form and the stable key's word come from the same list, so
//! they cannot drift apart.
//!
//! [`named!`] defines a report-only enum: it is written, never read. [`named_all!`] adds the
//! `ALL` list a set that must be complete iterates — only [`Class`](crate::compare::cases::Class)
//! needs one today. [`named_readable!`] adds the deserialiser a committed file needs, which is
//! `Category` alone — the decisions are the one enum a run parses rather than prints.

/// Define a write-only enum whose report word is stated once.
macro_rules! named {
    (
        $(#[$meta:meta])*
        $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $word:literal
            ),+ $(,)?
        }
    ) => {
        named!(@enum $(#[$meta])* $name { $($(#[$variant_meta])* $variant => $word),+ });
        named!(@serialize $name { $($variant => $word),+ });
    };

    (@enum $(#[$meta:meta])* $name:ident { $($(#[$variant_meta:meta])* $variant:ident => $word:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )+
        }

        impl $name {
            /// Its word in the report and in a stable key.
            #[must_use]
            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $word,)+
                }
            }
        }
    };

    (@all $name:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        impl $name {
            /// Every variant, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];
        }
    };

    (@serialize $name:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
    };

    (@deserialize $name:ident { $($variant:ident => $word:literal),+ $(,)? }) => {
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let word = String::deserialize(deserializer)?;
                match word.as_str() {
                    $($word => Ok(Self::$variant),)+
                    other => Err(serde::de::Error::unknown_variant(other, &[$($word),+])),
                }
            }
        }
    };
}

/// Define an enum that is both written and read, such as the decisions' category.
macro_rules! named_readable {
    (
        $(#[$meta:meta])*
        $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $word:literal
            ),+ $(,)?
        }
    ) => {
        $crate::compare::words::named!(@enum $(#[$meta])* $name { $($(#[$variant_meta])* $variant => $word),+ });
        $crate::compare::words::named!(@serialize $name { $($variant => $word),+ });
        $crate::compare::words::named!(@deserialize $name { $($variant => $word),+ });
    };
}

/// Define a write-only enum that also states its complete `ALL` list.
macro_rules! named_all {
    (
        $(#[$meta:meta])*
        $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident => $word:literal
            ),+ $(,)?
        }
    ) => {
        $crate::compare::words::named!(@enum $(#[$meta])* $name { $($(#[$variant_meta])* $variant => $word),+ });
        $crate::compare::words::named!(@all $name { $($variant => $word),+ });
        $crate::compare::words::named!(@serialize $name { $($variant => $word),+ });
    };
}

pub(crate) use {named, named_all, named_readable};
