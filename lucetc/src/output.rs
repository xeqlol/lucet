use crate::function::write_function_manifest;
use crate::name::Name;
use crate::stack_probe;
use crate::traps::write_trap_tables;
use cranelift_codegen::{ir, isa};
use cranelift_module::ModuleFunction;
use cranelift_faerie::{FaerieBackend, FaerieProduct};
use faerie::{Artifact, Decl};
use failure::{format_err, Error, ResultExt};
use lucet_module_data::FunctionSpec;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

pub struct CraneliftFuncs {
    funcs: HashMap<Name, ir::Function>,
    isa: Box<dyn isa::TargetIsa>,
}

impl CraneliftFuncs {
    pub fn new(funcs: HashMap<Name, ir::Function>, isa: Box<isa::TargetIsa>) -> Self {
        Self { funcs, isa }
    }
    /// This outputs a .clif file
    pub fn write<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        use cranelift_codegen::write_function;
        let mut buffer = String::new();
        for (n, func) in self.funcs.iter() {
            buffer.push_str(&format!("; {}\n", n.symbol()));
            write_function(&mut buffer, func, Some(self.isa.as_ref()))
                .context(format_err!("writing func {:?}", n))?
        }
        let mut file = File::create(path)?;
        file.write_all(buffer.as_bytes())?;
        Ok(())
    }
}

fn write_code_metadata(
    code_metadata: &lucet_module_data::CodeMetadata,
    output: &mut Artifact,
) -> Result<(), Error> {
    use byteorder::LittleEndian;
    use byteorder::WriteBytesExt;
    let serialized_metadata = code_metadata.serialize()?;

    let mut metadata_len_buf: Vec<u8> = Vec::new();
    metadata_len_buf
        .write_u32::<LittleEndian>(serialized_metadata.len() as u32)
        .unwrap();
    output
        .declare("lucet_code_metadata_len", Decl::data().global())
        .context("declaring lucet_code_metadata_len")?;
    output
        .define("lucet_code_metadata_len", metadata_len_buf)
        .context("defining lucet_code_metadata_len")?;

    output
        .declare("lucet_code_metadata", Decl::data().global())
        .context("declaring lucet_code_metadata")?;
    output
        .define("lucet_code_metadata", serialized_metadata)
        .context("defining lucet_code_metadata")?;

    Ok(())
}

pub struct ObjectFile {
    artifact: Artifact,
}
impl ObjectFile {
    pub fn new(mut product: FaerieProduct, mut function_manifest: Vec<(String, FunctionSpec)>) -> Result<Self, Error> {
        stack_probe::declare_and_define(&mut product)?;

        // stack_probe::declare_and_define adds a new function into `product`, but
        // function_manifest was already constructed from all defined functions -
        // so we have to add a new entry to `function_manifest` for the stack probe
        function_manifest.push((
            stack_probe::STACK_PROBE_SYM.to_string(),
            FunctionSpec::new(0, stack_probe::STACK_PROBE_BINARY.len() as u32)
        ));

        let trap_manifest = &product
            .trap_manifest
            .expect("trap manifest will be present");

        write_function_manifest(&function_manifest, &mut product.artifact)?;
        let internal_trap_manifest = write_trap_tables(trap_manifest, &function_manifest, &mut product.artifact)?;

        let code_metadata = lucet_module_data::CodeMetadata {
            trap_manifest: internal_trap_manifest,
        };

        write_code_metadata(&code_metadata, &mut product.artifact)?;
        Ok(Self {
            artifact: product.artifact,
        })
    }
    pub fn write<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        let _ = path.as_ref().file_name().ok_or(format_err!(
            "path {:?} needs to have filename",
            path.as_ref()
        ));
        let file = File::create(path)?;
        self.artifact.write(file)?;
        Ok(())
    }
}