use std::ptr::NonNull;

use crate::sys::exports::{ExportError, Exportable};
use crate::sys::externals::Extern;
use crate::sys::store::{AsStoreMut, AsStoreRef};
use crate::sys::value::Value;
use crate::sys::GlobalType;
use crate::sys::Mutability;
use crate::sys::RuntimeError;
use wasmer_vm::{InternalStoreHandle, StoreHandle, VMExtern, VMGlobal, VMGlobalDefinition};

/// A WebAssembly `global` instance.
///
/// A global instance is the runtime representation of a global variable.
/// It consists of an individual value and a flag indicating whether it is mutable.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#global-instances>
#[derive(Debug, Clone)]
pub struct Global {
    handle: StoreHandle<VMGlobal>,
}

impl Global {
    /// Create a new `Global` with the initial value [`Value`].
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Mutability, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new(&mut store, Value::I32(1));
    ///
    /// assert_eq!(g.get(&mut store), Value::I32(1));
    /// assert_eq!(g.ty(&mut store).mutability, Mutability::Const);
    /// ```
    pub fn new(store: &mut impl AsStoreMut, val: Value) -> Self {
        Self::from_value(store, val, Mutability::Const).unwrap()
    }

    /// Create a mutable `Global` with the initial value [`Value`].
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Mutability, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new_mut(&mut store, Value::I32(1));
    ///
    /// assert_eq!(g.get(&mut store), Value::I32(1));
    /// assert_eq!(g.ty(&mut store).mutability, Mutability::Var);
    /// ```
    pub fn new_mut(store: &mut impl AsStoreMut, val: Value) -> Self {
        Self::from_value(store, val, Mutability::Var).unwrap()
    }

    /// Create a `Global` with the initial value [`Value`] and the provided [`Mutability`].
    fn from_value(
        store: &mut impl AsStoreMut,
        val: Value,
        mutability: Mutability,
    ) -> Result<Self, RuntimeError> {
        if !val.is_from_store(store) {
            return Err(RuntimeError::new(
                "cross-`Context` values are not supported",
            ));
        }
        let global = VMGlobal::new(GlobalType {
            mutability,
            ty: val.ty(),
        });
        unsafe {
            global.vmglobal().as_mut().val = val.as_raw(store);
        }

        Ok(Self {
            handle: StoreHandle::new(store.objects_mut(), global),
        })
    }

    /// Returns the [`GlobalType`] of the `Global`.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Mutability, Store, Type, Value, GlobalType};
    /// # let mut store = Store::default();
    /// #
    /// let c = Global::new(&mut store, Value::I32(1));
    /// let v = Global::new_mut(&mut store, Value::I64(1));
    ///
    /// assert_eq!(c.ty(&mut store), GlobalType::new(Type::I32, Mutability::Const));
    /// assert_eq!(v.ty(&mut store), GlobalType::new(Type::I64, Mutability::Var));
    /// ```
    pub fn ty(&self, store: &impl AsStoreRef) -> GlobalType {
        *self.handle.get(store.as_store_ref().objects()).ty()
    }

    #[cfg(feature = "compiler")]
    /// Retrieves the current value [`Value`] that the Global has.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new(&mut store, Value::I32(1));
    ///
    /// assert_eq!(g.get(&mut store), Value::I32(1));
    /// ```
    pub fn get(&self, store: &mut impl AsStoreMut) -> Value {
        unsafe {
            let global = self.handle.get(store.as_store_ref().objects());
            let raw = global.vmglobal().as_ref().val;
            let ty = global.ty().ty;
            Value::from_raw(store, ty, raw)
        }
    }

    /// Sets a custom value [`Value`] to the runtime Global.
    ///
    /// # Example
    ///
    /// ```
    /// # use wasmer::{Global, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new_mut(&mut store, Value::I32(1));
    ///
    /// assert_eq!(g.get(&mut store), Value::I32(1));
    ///
    /// g.set(&mut store, Value::I32(2));
    ///
    /// assert_eq!(g.get(&mut store), Value::I32(2));
    /// ```
    ///
    /// # Errors
    ///
    /// Trying to mutate a immutable global will raise an error:
    ///
    /// ```should_panic
    /// # use wasmer::{Global, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new(&mut store, Value::I32(1));
    ///
    /// g.set(&mut store, Value::I32(2)).unwrap();
    /// ```
    ///
    /// Trying to set a value of a incompatible type will raise an error:
    ///
    /// ```should_panic
    /// # use wasmer::{Global, Store, Value};
    /// # let mut store = Store::default();
    /// #
    /// let g = Global::new(&mut store, Value::I32(1));
    ///
    /// // This results in an error: `RuntimeError`.
    /// g.set(&mut store, Value::I64(2)).unwrap();
    /// ```
    pub fn set(&self, store: &mut impl AsStoreMut, val: Value) -> Result<(), RuntimeError> {
        if !val.is_from_store(store) {
            return Err(RuntimeError::new(
                "cross-`Context` values are not supported",
            ));
        }
        let global = self.handle.get_mut(store.objects_mut());
        if global.ty().mutability != Mutability::Var {
            return Err(RuntimeError::new("Attempted to set an immutable global"));
        }
        if val.ty() != global.ty().ty {
            return Err(RuntimeError::new(format!(
                "Attempted to operate on a global of type {expected} as a global of type {found}",
                expected = global.ty().ty,
                found = val.ty(),
            )));
        }
        unsafe {
            let mut dest = global.vmglobal();
            dest.as_mut().val = val.as_raw(store);
        }
        Ok(())
    }

    pub(crate) fn from_vm_extern(
        store: &mut impl AsStoreMut,
        internal: InternalStoreHandle<VMGlobal>,
    ) -> Self {
        Self {
            handle: unsafe {
                StoreHandle::from_internal(store.as_store_ref().objects().id(), internal)
            },
        }
    }

    /// Checks whether this `Global` can be used with the given context.
    pub fn is_from_store(&self, store: &impl AsStoreRef) -> bool {
        self.handle.store_id() == store.as_store_ref().objects().id()
    }

    pub(crate) fn to_vm_extern(&self) -> VMExtern {
        VMExtern::Global(self.handle.internal_handle())
    }

    /// Gets the underlying VM pointer
    pub unsafe fn vmglobal(&self, store: &mut impl AsStoreMut) -> NonNull<VMGlobalDefinition> {
        self.handle.get_mut(store.objects_mut()).vmglobal()
    }
}

impl std::cmp::PartialEq for Global {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl std::cmp::Eq for Global {}

impl<'a> Exportable<'a> for Global {
    fn get_self_from_extern(_extern: &'a Extern) -> Result<&'a Self, ExportError> {
        match _extern {
            Extern::Global(global) => Ok(global),
            _ => Err(ExportError::IncompatibleType),
        }
    }
}
