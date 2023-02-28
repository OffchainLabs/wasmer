//use crate::js::externals::Function;
// use crate::store::{Store, StoreObject};
// use crate::js::RuntimeError;
use crate::imports::Imports;
use crate::instance::Instance;
use crate::js::instance::Instance as JsInstance;
use crate::js::vm::{VMFunction, VMGlobal, VMMemory, VMTable};
use crate::js::wasm_bindgen_polyfill::Global as JsGlobal;
use crate::store::{AsStoreMut, AsStoreRef};
use crate::value::Value;
use crate::Type;
use crate::{Extern, Function, Global, Memory, Table};
use js_sys::Function as JsFunction;
use js_sys::WebAssembly::{Memory as JsMemory, Table as JsTable};
use std::collections::HashMap;
use wasm_bindgen::JsCast;
use wasm_bindgen::{JsError, JsValue};
use wasmer_types::ExternType;

/// Convert the given type to a [`JsValue`].
pub trait AsJs: Sized {
    /// The inner definition type from this Javascript object
    type DefinitionType;
    /// Convert the given type to a [`JsValue`].
    fn as_jsvalue(&self, store: &impl AsStoreRef) -> JsValue;
    /// Convert the given type to a [`JsValue`].
    fn from_jsvalue(
        store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &JsValue,
    ) -> Result<Self, JsError>;
}

#[inline]
pub fn param_from_js(ty: &Type, js_val: &JsValue) -> Value {
    match ty {
        Type::I32 => Value::I32(js_val.as_f64().unwrap() as _),
        Type::I64 => Value::I64(js_val.as_f64().unwrap() as _),
        Type::F32 => Value::F32(js_val.as_f64().unwrap() as _),
        Type::F64 => Value::F64(js_val.as_f64().unwrap()),
        t => unimplemented!(
            "The type `{:?}` is not yet supported in the JS Function API",
            t
        ),
    }
}

impl AsJs for Value {
    type DefinitionType = Type;

    fn as_jsvalue(&self, _store: &impl AsStoreRef) -> JsValue {
        match self {
            Self::I32(i) => JsValue::from_f64(*i as f64),
            Self::I64(i) => JsValue::from_f64(*i as f64),
            Self::F32(f) => JsValue::from_f64(*f as f64),
            Self::F64(f) => JsValue::from_f64(*f),
            Self::V128(f) => JsValue::from_f64(*f as f64),
            Self::FuncRef(Some(func)) => func.0.handle.function.clone().into(),
            Self::FuncRef(None) => JsValue::null(),
            Self::ExternRef(_) => unimplemented!(),
        }
    }

    fn from_jsvalue(
        _store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &JsValue,
    ) -> Result<Self, JsError> {
        Ok(param_from_js(type_, value))
    }
}

impl AsJs for wasmer_types::RawValue {
    type DefinitionType = Type;

    fn as_jsvalue(&self, _store: &impl AsStoreRef) -> JsValue {
        unsafe { JsValue::from_f64(self.into()) }
    }

    fn from_jsvalue(
        _store: &mut impl AsStoreMut,
        type_: &Self::DefinitionType,
        value: &JsValue,
    ) -> Result<Self, JsError> {
        unimplemented!();
    }
}

impl AsJs for Imports {
    type DefinitionType = crate::module::Module;

    // Annotation is here to prevent spurious IDE warnings.
    #[allow(unused_unsafe)]
    fn as_jsvalue(&self, store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
        let imports_object = js_sys::Object::new();
        for (namespace, name, extern_) in self.iter() {
            let val = unsafe { js_sys::Reflect::get(&imports_object, &namespace.into()).unwrap() };
            if !val.is_undefined() {
                // If the namespace is already set

                // Annotation is here to prevent spurious IDE warnings.
                #[allow(unused_unsafe)]
                unsafe {
                    js_sys::Reflect::set(
                        &val,
                        &name.into(),
                        &extern_.as_jsvalue(&store.as_store_ref()),
                    )
                    .unwrap();
                }
            } else {
                // If the namespace doesn't exist
                let import_namespace = js_sys::Object::new();
                #[allow(unused_unsafe)]
                unsafe {
                    js_sys::Reflect::set(
                        &import_namespace,
                        &name.into(),
                        &extern_.as_jsvalue(&store.as_store_ref()),
                    )
                    .unwrap();
                    js_sys::Reflect::set(
                        &imports_object,
                        &namespace.into(),
                        &import_namespace.into(),
                    )
                    .unwrap();
                }
            }
        }
        imports_object.into()
    }

    fn from_jsvalue(
        store: &mut impl AsStoreMut,
        module: &Self::DefinitionType,
        value: &JsValue,
    ) -> Result<Self, JsError> {
        let module_imports: HashMap<(String, String), ExternType> = module
            .imports()
            .map(|import| {
                (
                    (import.module().to_string(), import.name().to_string()),
                    import.ty().clone(),
                )
            })
            .collect::<HashMap<(String, String), ExternType>>();

        let mut map: HashMap<(String, String), Extern> = HashMap::new();
        let object: js_sys::Object = value.clone().into();
        for module_entry in js_sys::Object::entries(&object).iter() {
            let module_entry: js_sys::Array = module_entry.into();
            let module_name = module_entry.get(0).as_string().unwrap().to_string();
            let module_import_object: js_sys::Object = module_entry.get(1).into();
            for import_entry in js_sys::Object::entries(&module_import_object).iter() {
                let import_entry: js_sys::Array = import_entry.into();
                let import_name = import_entry.get(0).as_string().unwrap().to_string();
                let import_js: wasm_bindgen::JsValue = import_entry.get(1);
                let key = (module_name.clone(), import_name);
                let extern_type = module_imports.get(&key).unwrap();
                let extern_ = Extern::from_jsvalue(store, extern_type, &import_js)?;
                map.insert(key, extern_);
            }
        }

        Ok(Self { map })
    }
}

impl AsJs for Extern {
    type DefinitionType = ExternType;

    fn as_jsvalue(&self, _store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
        match self {
            Self::Memory(memory) => memory.0.handle.memory.clone().into(),
            Self::Function(function) => function.0.handle.function.clone().into(),
            Self::Table(table) => table.0.handle.table.clone().into(),
            Self::Global(global) => global.0.handle.global.clone().into(),
        }
    }

    fn from_jsvalue(
        store: &mut impl AsStoreMut,
        extern_type: &Self::DefinitionType,
        val: &JsValue,
    ) -> Result<Self, JsError> {
        // Note: this function do a soft check over the type
        // We only check the "kind" of Extern, but nothing else
        match extern_type {
            ExternType::Memory(memory_type) => {
                if val.is_instance_of::<JsMemory>() {
                    Ok(Self::Memory(Memory::from_vm_extern(
                        store,
                        VMMemory::new(
                            val.clone().unchecked_into::<JsMemory>(),
                            memory_type.clone(),
                        ),
                    )))
                } else {
                    Err(JsError::new(&format!(
                        "Extern expect to be of type Memory, but received {:?}",
                        val
                    )))
                }
            }
            ExternType::Global(global_type) => {
                if val.is_instance_of::<JsGlobal>() {
                    Ok(Self::Global(Global::from_vm_extern(
                        store,
                        VMGlobal::new(
                            val.clone().unchecked_into::<JsGlobal>(),
                            global_type.clone(),
                        ),
                    )))
                } else {
                    Err(JsError::new(&format!(
                        "Extern expect to be of type Global, but received {:?}",
                        val
                    )))
                }
            }
            ExternType::Function(function_type) => {
                if val.is_instance_of::<JsFunction>() {
                    Ok(Self::Function(Function::from_vm_extern(
                        store,
                        VMFunction::new(
                            val.clone().unchecked_into::<JsFunction>(),
                            function_type.clone(),
                        ),
                    )))
                } else {
                    Err(JsError::new(&format!(
                        "Extern expect to be of type Function, but received {:?}",
                        val
                    )))
                }
            }
            ExternType::Table(table_type) => {
                if val.is_instance_of::<JsTable>() {
                    Ok(Self::Table(Table::from_vm_extern(
                        store,
                        VMTable::new(val.clone().unchecked_into::<JsTable>(), table_type.clone()),
                    )))
                } else {
                    Err(JsError::new(&format!(
                        "Extern expect to be of type Table, but received {:?}",
                        val
                    )))
                }
            }
        }
    }
}

impl AsJs for Instance {
    type DefinitionType = crate::module::Module;
    fn as_jsvalue(&self, store: &impl AsStoreRef) -> wasm_bindgen::JsValue {
        self._inner._handle.clone().into()
    }

    fn from_jsvalue(
        mut store: &mut impl AsStoreMut,
        module: &Self::DefinitionType,
        value: &JsValue,
    ) -> Result<Self, JsError> {
        let js_instance: js_sys::WebAssembly::Instance = value.clone().into();
        let (instance, exports) = JsInstance::from_module_and_instance(store, module, js_instance)
            .map_err(|e| JsError::new(&format!("Can't get the instance: {:?}", e)))?;
        Ok(Instance {
            _inner: instance,
            module: module.clone(),
            exports,
        })
    }
}
