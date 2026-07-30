macro_rules! operation_schema {
    (@parse
        [$($types:tt)*]
        [$($operation:tt)*]
        [$($success:tt)*]
        [$($metadata:tt)*]
        [$($definition:tt)*]
        $variant:ident($name:literal) {
            args {
                $(#[$args_attribute:meta])*
                $args_visibility:vis struct $args:ident { $($args_fields:tt)* }
            }
            result {
                $(#[$result_attribute:meta])*
                $result_visibility:vis struct $result:ident { $($result_fields:tt)* }
            }
            $(queries: [$($query:ident),* $(,)?],)?
            $(fetches: [$($fetch:ident),* $(,)?],)?
            $(mutates: [$($mutate:ident),* $(,)?],)?
        }
        $($rest:tt)*
    ) => {
        operation_schema!(@push
            [$($types)*
                $(#[$args_attribute])*
                $args_visibility struct $args { $($args_fields)* }

                $(#[$result_attribute])*
                $result_visibility struct $result { $($result_fields)* }
            ]
            [$($operation)*]
            [$($success)*]
            [$($metadata)*]
            [$($definition)*]
            $variant($name, $args) => $result {
                queries: [$($($query),*)?],
                fetches: [$($($fetch),*)?],
                mutates: [$($($mutate),*)?],
            }
            $($rest)*
        );
    };

    (@parse
        [$($types:tt)*]
        [$($operation:tt)*]
        [$($success:tt)*]
        [$($metadata:tt)*]
        [$($definition:tt)*]
        $variant:ident($name:literal) {
            args {
                $(#[$args_attribute:meta])*
                $args_visibility:vis struct $args:ident { $($args_fields:tt)* }
            }
            result Box {
                $(#[$result_attribute:meta])*
                $result_visibility:vis struct $result:ident { $($result_fields:tt)* }
            }
            $(queries: [$($query:ident),* $(,)?],)?
            $(fetches: [$($fetch:ident),* $(,)?],)?
            $(mutates: [$($mutate:ident),* $(,)?],)?
        }
        $($rest:tt)*
    ) => {
        operation_schema!(@push
            [$($types)*
                $(#[$args_attribute])*
                $args_visibility struct $args { $($args_fields)* }

                $(#[$result_attribute])*
                $result_visibility struct $result { $($result_fields)* }
            ]
            [$($operation)*]
            [$($success)*]
            [$($metadata)*]
            [$($definition)*]
            $variant($name, $args) => Box<$result> {
                queries: [$($($query),*)?],
                fetches: [$($($fetch),*)?],
                mutates: [$($($mutate),*)?],
            }
            $($rest)*
        );
    };

    (@parse
        [$($types:tt)*]
        [$($operation:tt)*]
        [$($success:tt)*]
        [$($metadata:tt)*]
        [$($definition:tt)*]
        $variant:ident($name:literal) {
            args {
                $(#[$args_attribute:meta])*
                $args_visibility:vis struct $args:ident { $($args_fields:tt)* }
            }
            result $result:ident
            $(queries: [$($query:ident),* $(,)?],)?
            $(fetches: [$($fetch:ident),* $(,)?],)?
            $(mutates: [$($mutate:ident),* $(,)?],)?
        }
        $($rest:tt)*
    ) => {
        operation_schema!(@push
            [$($types)*
                $(#[$args_attribute])*
                $args_visibility struct $args { $($args_fields)* }
            ]
            [$($operation)*]
            [$($success)*]
            [$($metadata)*]
            [$($definition)*]
            $variant($name, $args) => $result {
                queries: [$($($query),*)?],
                fetches: [$($($fetch),*)?],
                mutates: [$($($mutate),*)?],
            }
            $($rest)*
        );
    };

    (@parse
        [$($types:tt)*]
        [$($operation:tt)*]
        [$($success:tt)*]
        [$($metadata:tt)*]
        [$($definition:tt)*]
        $variant:ident($name:literal) {
            args {
                $(#[$args_attribute:meta])*
                $args_visibility:vis struct $args:ident { $($args_fields:tt)* }
            }
            result Box<$result:ident>
            $(queries: [$($query:ident),* $(,)?],)?
            $(fetches: [$($fetch:ident),* $(,)?],)?
            $(mutates: [$($mutate:ident),* $(,)?],)?
        }
        $($rest:tt)*
    ) => {
        operation_schema!(@push
            [$($types)*
                $(#[$args_attribute])*
                $args_visibility struct $args { $($args_fields)* }
            ]
            [$($operation)*]
            [$($success)*]
            [$($metadata)*]
            [$($definition)*]
            $variant($name, $args) => Box<$result> {
                queries: [$($($query),*)?],
                fetches: [$($($fetch),*)?],
                mutates: [$($($mutate),*)?],
            }
            $($rest)*
        );
    };

    (@push
        [$($types:tt)*]
        [$($operation:tt)*]
        [$($success:tt)*]
        [$($metadata:tt)*]
        [$($definition:tt)*]
        $variant:ident($name:literal, $args:ident) => $result:ident {
            queries: [$($query:ident),* $(,)?],
            fetches: [$($fetch:ident),* $(,)?],
            mutates: [$($mutate:ident),* $(,)?] $(,)?
        }
        $($rest:tt)*
    ) => {
        operation_schema!(@parse
            [$($types)*]
            [$($operation)* $variant($args),]
            [$($success)* $variant($result),]
            [$($metadata)* Self::$variant(_) => OperationMetadata {
                queries: &[$(OperationResource::$query),*],
                fetches: &[$(OperationResource::$fetch),*],
                mutates: &[$(OperationResource::$mutate),*],
            },]
            [$($definition)* OperationDefinition {
                name: $name,
                metadata: OperationMetadata {
                    queries: &[$(OperationResource::$query),*],
                    fetches: &[$(OperationResource::$fetch),*],
                    mutates: &[$(OperationResource::$mutate),*],
                },
            },]
            $($rest)*
        );
    };

    (@push
        [$($types:tt)*]
        [$($operation:tt)*]
        [$($success:tt)*]
        [$($metadata:tt)*]
        [$($definition:tt)*]
        $variant:ident($name:literal, $args:ident) => Box<$result:ident> {
            queries: [$($query:ident),* $(,)?],
            fetches: [$($fetch:ident),* $(,)?],
            mutates: [$($mutate:ident),* $(,)?] $(,)?
        }
        $($rest:tt)*
    ) => {
        operation_schema!(@parse
            [$($types)*]
            [$($operation)* $variant($args),]
            [$($success)* $variant(Box<$result>),]
            [$($metadata)* Self::$variant(_) => OperationMetadata {
                queries: &[$(OperationResource::$query),*],
                fetches: &[$(OperationResource::$fetch),*],
                mutates: &[$(OperationResource::$mutate),*],
            },]
            [$($definition)* OperationDefinition {
                name: $name,
                metadata: OperationMetadata {
                    queries: &[$(OperationResource::$query),*],
                    fetches: &[$(OperationResource::$fetch),*],
                    mutates: &[$(OperationResource::$mutate),*],
                },
            },]
            $($rest)*
        );
    };

    (@parse
        [$($types:tt)*]
        [$($operation:tt)*]
        [$($success:tt)*]
        [$($metadata:tt)*]
        [$($definition:tt)*]
    ) => {
        $($types)*

        #[derive(PartialEq, Eq, Serialize, Deserialize, Type)]
        #[serde(tag = "op", content = "args", rename_all = "camelCase")]
        pub enum Operation {
            $($operation)*
        }

        impl Operation {
            pub const fn metadata(&self) -> OperationMetadata {
                match self {
                    $($metadata)*
                }
            }

            pub const fn definitions() -> &'static [OperationDefinition] {
                &[$($definition)*]
            }
        }

        #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
        #[serde(tag = "op", content = "result", rename_all = "camelCase")]
        pub enum OperationSuccess {
            $($success)*
        }
    };

    ($($input:tt)*) => {
        operation_schema!(@parse [] [] [] [] [] $($input)*);
    };
}

pub(crate) use operation_schema;
