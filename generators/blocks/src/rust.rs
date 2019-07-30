use super::*;
use std::process::Command;

pub fn generate_rust_code(input: &str, output: &str) -> Result<(), Error> {
    info!(
        "Writing Rust `Block` enum and data structs to {} using native input report {}",
        output, input,
    );

    let in_file = File::open(input)?;
    let mut out_file = File::create(output)?;

    info!("Parsing data file");
    let report: BlockReport = serde_json::from_reader(BufReader::new(&in_file))?;
    info!("Parsing successful");

    let mut enum_entries = vec![];
    //let mut name_fn_entries = vec![];
    //let mut from_name_and_props_fn_entries = vec![];
    let mut data_structs = vec![];
    let mut property_enums = vec![];

    let mut native_type_id_entries = vec![];

    info!("Generating code");

    let mut count = 0;
    for (block_name, block) in &report.blocks {
        generate_block_code(
            block,
            block_name,
            &mut property_enums,
            &mut data_structs,
            &mut enum_entries,
            &mut native_type_id_entries,
            count,
        );

        count += 1;
    }

    let known_enums = generate_known_enums();

    let block = quote! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Block {
            #(#enum_entries),*
        }

        impl Block {
            pub fn native_type_id(&self) -> usize {
                match self {
                    #(#native_type_id_entries),*
                }
            }
        }
    };

    let value = quote! {
        pub trait Value {
            fn value(&self) -> usize;
        }

        impl Value for i32 {
            fn value(&self) -> usize {
                *self as usize
            }
        }

        impl Value for bool {
            fn value(&self) -> usize {
                match *self {
                    true => 1,
                    false => 0,
                }
            }
        }
    };

    let from_snake_case = quote! {
        pub trait FromSnakeCase {
            fn from_snake_case(val: &str) -> Option<Self>
                where Self: Sized;
        }

        impl FromSnakeCase for i32 {
            fn from_snake_case(val: &str) -> Option<Self> {
                use std::str::FromStr;
                match i32::from_str(val) {
                    Ok(x) => Some(x),
                    Err(_) => None,
                }
            }
        }

        impl FromSnakeCase for bool {
            fn from_snake_case(val: &str) -> Option<Self> {
                use std::str::FromStr;
                match bool::from_str(val) {
                    Ok(x) => Some(x),
                    Err(_) => None,
                }
            }
        }
    };

    let to_snake_case = quote! {
        pub trait ToSnakeCase {
            fn to_snake_case(&self) -> String;
        }

        impl ToSnakeCase for i32 {
            fn to_snake_case(&self) -> String {
                self.to_string()
            }
        }

        impl ToSnakeCase for bool {
            fn to_snake_case(&self) -> String {
                self.to_string()
            }
        }
    };

    let result = quote! {
        use feather_codegen::{ToSnakeCase, FromSnakeCase};
        use std::collections::HashMap;

        #block
        #value
        #from_snake_case
        #to_snake_case
        #(#data_structs)*
        #(#property_enums)*
        #known_enums
    };

    out_file.write(b"//! This file was generated by /generators/blocks\n")?;
    out_file.write(result.to_string().as_bytes())?;
    out_file.flush()?;

    info!("Successfully wrote code to {}", output);

    info!("Formatting code with rustfmt");

    run_rustfmt(output)?;

    info!("Success");

    Ok(())
}

fn run_rustfmt(file: &str) -> Result<(), Error> {
    Command::new("rustfmt").args(&[file]).output()?;

    Ok(())
}

fn generate_block_code(
    block: &Block,
    block_name: &String,
    property_enums: &mut Vec<TokenStream>,
    data_structs: &mut Vec<TokenStream>,
    enum_entries: &mut Vec<TokenStream>,
    native_type_id_entries: &mut Vec<TokenStream>,
    count: usize,
) {
    let variant_name = block_name[10..].to_camel_case();
    let variant_ident = Ident::new(&variant_name, Span::call_site());

    // If block has properties, we need to create a
    // data struct for the block and include it in the
    // enum variant.
    if let Some(props) = block.properties.clone() {
        create_block_data_struct(&variant_name, &props, property_enums, data_structs);
        let data_struct_ident = Ident::new(&format!("{}Data", variant_name), Span::call_site());
        enum_entries.push(quote! {
            #variant_ident(#data_struct_ident)
        });
        native_type_id_entries.push(quote! {
            Block::#variant_ident(_) => #count
        });
    } else {
        enum_entries.push(quote! {
            #variant_ident
        });

        native_type_id_entries.push(quote! {
            Block::#variant_ident => #count
        });
    }
}

fn create_block_data_struct(
    variant_name: &str,
    props: &BlockProperties,
    property_enums: &mut Vec<TokenStream>,
    data_structs: &mut Vec<TokenStream>,
) {
    let mut data_struct_entries = vec![];
    let mut from_map_entries = vec![];
    let mut to_map_entries = vec![];

    for (prop_name_str, possible_values) in &props.props {
        let ty = PropValueType::guess_from_value(&possible_values[0]);

        // If type is a custom enum, create the enum type
        if ty == PropValueType::Enum {
            create_property_enum(variant_name, prop_name_str, possible_values, property_enums);
        }

        let enum_name = format!("{}{}", variant_name, prop_name_str.to_camel_case());

        let ty_ident = Ident::new(
            match ty {
                PropValueType::Bool => "bool",
                PropValueType::I32 => "i32",
                PropValueType::Part => "Part",
                PropValueType::Hinge => "Hinge",
                PropValueType::Shape => "Shape",
                PropValueType::Half => "Half",
                PropValueType::Axis => "Axis",
                PropValueType::Face => "Face",
                PropValueType::Facing => "Facing",
                PropValueType::Enum => &enum_name,
            },
            Span::call_site(),
        );

        let field_name = Ident::new(
            correct_variable_name(prop_name_str.as_str()),
            Span::call_site(),
        );

        let entry = quote! {
            #field_name: #ty_ident
        };
        data_struct_entries.push(entry);

        from_map_entries.push(quote! {
            #field_name: #ty_ident::from_snake_case(map.get(#prop_name_str)?)?
        });

        to_map_entries.push(quote! {
            m.insert(#prop_name_str.to_string(), self.#field_name.to_snake_case());
        });
    }

    let data_ident = Ident::new(&format!("{}Data", variant_name), Span::call_site());

    let value_impl = generate_value_implementation(&data_ident, props);

    let data_struct = quote! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct #data_ident {
            #(#data_struct_entries),*
        }

        impl #data_ident {
            pub fn from_map(map: &HashMap<String, String>) -> Option<Self> {
                Some(Self {
                    #(#from_map_entries),*
                })
            }

            pub fn to_map(&self) -> HashMap<String, String> {
                let mut m = HashMap::new();
                #(#to_map_entries)*
                m
            }
        }

        #value_impl
    };

    data_structs.push(data_struct);
}

fn create_property_enum(
    variant_name: &str,
    prop_name: &str,
    possible_values: &Vec<String>,
    property_enums: &mut Vec<TokenStream>,
) {
    let enum_ident = Ident::new(
        &format!("{}{}", variant_name, prop_name.to_camel_case()),
        Span::call_site(),
    );

    let mut enum_variants = vec![];

    for possible_value_str in possible_values {
        let possible_value = Ident::new(
            possible_value_str.to_camel_case().as_str(),
            Span::call_site(),
        );
        enum_variants.push(quote! {
              #possible_value
        });
    }

    let en = quote! {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ToSnakeCase, FromSnakeCase)]
        pub enum #enum_ident {
            #(#enum_variants),*
        }

        impl Value for #enum_ident {
            fn value(&self) -> usize {
                *self as usize
            }
        }
    };

    property_enums.push(en);
}

fn correct_variable_name(name: &str) -> &str {
    match name {
        "type" => "ty",
        "in" => "_in",
        name => name,
    }
}

fn generate_known_enums() -> TokenStream {
    let facing = quote! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ToSnakeCase, FromSnakeCase)]
    pub enum Facing {
        North,
        South,
        East,
        West,
        Up,
        Down,
    }

    impl Value for Facing {
        fn value(&self) -> usize {
            *self as usize
        }
    }
    };
    let axis = quote! {

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ToSnakeCase, FromSnakeCase)]
    pub enum Axis {
        X,
        Y,
        Z,
    }

    impl Value for Axis {
        fn value(&self) -> usize {
            *self as usize
        }
    }
    };

    let half = quote! {

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ToSnakeCase, FromSnakeCase)]
    pub enum Half {
        Upper,
        Lower,
        Top,
        Bottom,
    }

    impl Value for Half {
        fn value(&self) -> usize {
            *self as usize
        }
    }
    };

    let face = quote! {

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ToSnakeCase, FromSnakeCase)]
    pub enum Face {
        Floor,
        Wall,
        Ceiling,
    }

    impl Value for Face {
        fn value(&self) -> usize {
            *self as usize
        }
    }
    };

    let shape = quote! {

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ToSnakeCase, FromSnakeCase)]
    pub enum Shape {
        Straight,
        InnerLeft,
        InnerRight,
        OuterLeft,
        AscendingNorth,
        AscendingSouth,
        AscendingEast,
        AscendingWest,
        NorthEast,
        NorthWest,
        SouthEast,
        SouthWest,
        NorthSouth,
        EastWest,
    }

    impl Value for Shape {
        fn value(&self) -> usize {
            *self as usize
        }
    }
    };

    let hinge = quote! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ToSnakeCase, FromSnakeCase)]
    pub enum Hinge {
        Left,
        Right,
    }

    impl Value for Hinge {
        fn value(&self) -> usize {
            *self as usize
        }
    }
    };

    let part = quote! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, ToSnakeCase, FromSnakeCase)]
    pub enum Part {
        Head,
        Foot,
    }

    impl Value for Part {
        fn value(&self) -> usize {
            *self as usize
        }
    }
    };

    quote! {
        #facing
        #axis
        #half
        #face
        #hinge
        #shape
        #part
    }
}

const FACING_VALUES: [&'static str; 6] = ["north", "south", "east", "west", "up", "down"];

const AXIS_VALUES: [&'static str; 3] = ["x", "y", "z"];

const HALF_VALUES: [&'static str; 4] = ["upper", "lower", "top", "bottom"];

const FACE_VALUES: [&'static str; 3] = ["floor", "wall", "ceiling"];

const SHAPE_VALUES: [&'static str; 14] = [
    "straight",
    "inner_left",
    "inner_right",
    "outer_left",
    "ascending_north",
    "ascending_south",
    "ascending_east",
    "ascending_west",
    "north_east",
    "north_west",
    "south_east",
    "south_west",
    "north_south",
    "east_west",
];

const HINGE_VALUES: [&'static str; 2] = ["left", "right"];

const PART_VALUES: [&'static str; 2] = ["head", "foot"];

/// A property value type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PropValueType {
    Facing,
    Axis,
    Half,
    Face,
    Shape,
    Hinge,
    Part,
    Enum,
    I32,
    Bool,
}

impl PropValueType {
    fn guess_from_value(value: &str) -> Self {
        if FACING_VALUES.contains(&value) {
            PropValueType::Facing
        } else if AXIS_VALUES.contains(&value) {
            PropValueType::Axis
        } else if HALF_VALUES.contains(&value) {
            PropValueType::Half
        } else if FACE_VALUES.contains(&value) {
            PropValueType::Face
        } else if SHAPE_VALUES.contains(&value) {
            PropValueType::Shape
        } else if HINGE_VALUES.contains(&value) {
            PropValueType::Hinge
        } else if PART_VALUES.contains(&value) {
            PropValueType::Part
        } else if i32::from_str(value).is_ok() {
            PropValueType::I32
        } else if bool::from_str(value).is_ok() {
            PropValueType::Bool
        } else {
            PropValueType::Enum // Custom enum
        }
    }
}

/// Generates a `Value` implementation for
/// a data struct.
///
/// This uses a special algorithm to generate
/// consecutive values in constant time.
fn generate_value_implementation(
    data_struct_ident: &Ident,
    props: &BlockProperties,
) -> TokenStream {
    use crate::util::slice_product;

    let mut terms = vec![];

    let possible_value_lens: Vec<usize> = props
        .props
        .iter()
        .map(|(_, possible_values)| possible_values.len())
        .collect();

    for (count, (prop_name, _)) in props.props.iter().enumerate() {
        let multiplier = if count == props.props.len() - 1 {
            // This is the last property - just multiply by 1.
            1
        } else {
            slice_product(&possible_value_lens[count + 1..])
        };

        let prop_field = Ident::new(correct_variable_name(prop_name.as_str()), Span::call_site());

        terms.push(quote! {
            (self.#prop_field.value() * #multiplier)
        })
    }

    let result = quote! {
        impl Value for #data_struct_ident {
            fn value(&self) -> usize {
                #(#terms)+*
            }
        }
    };

    result
}
