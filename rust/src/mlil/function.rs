use core::hash::{Hash, Hasher};
use std::ffi::c_char;

use binaryninjacore_sys::*;

use crate::architecture::CoreArchitecture;
use crate::basicblock::BasicBlock;
use crate::function::{Function, Location};
use crate::rc::{Array, CoreArrayProvider, CoreArrayProviderInner, Ref, RefCountable};
use crate::string::BnStrCompatible;
use crate::types::{Conf, PossibleValueSet, Type, UserVariableValues, Variable};

use super::{MediumLevelILBlock, MediumLevelILInstruction, MediumLevelILLiftedInstruction};

pub struct MediumLevelILFunction {
    pub(crate) handle: *mut BNMediumLevelILFunction,
}

unsafe impl Send for MediumLevelILFunction {}
unsafe impl Sync for MediumLevelILFunction {}

impl Eq for MediumLevelILFunction {}
impl PartialEq for MediumLevelILFunction {
    fn eq(&self, rhs: &Self) -> bool {
        self.get_function().eq(&rhs.get_function())
    }
}

impl Hash for MediumLevelILFunction {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.get_function().hash(state)
    }
}

impl MediumLevelILFunction {
    pub(crate) unsafe fn ref_from_raw(handle: *mut BNMediumLevelILFunction) -> Ref<Self> {
        debug_assert!(!handle.is_null());

        Self { handle }.to_owned()
    }

    pub fn instruction_at<L: Into<Location>>(&self, loc: L) -> Option<MediumLevelILInstruction> {
        let loc: Location = loc.into();
        let arch_handle = loc.arch.unwrap();

        let expr_idx =
            unsafe { BNMediumLevelILGetInstructionStart(self.handle, arch_handle.0, loc.addr) };

        if expr_idx >= self.instruction_count() {
            None
        } else {
            Some(MediumLevelILInstruction::new(self.to_owned(), expr_idx))
        }
    }

    pub fn instruction_from_idx(&self, expr_idx: usize) -> MediumLevelILInstruction {
        MediumLevelILInstruction::new(self.to_owned(), expr_idx)
    }

    pub fn lifted_instruction_from_idx(&self, expr_idx: usize) -> MediumLevelILLiftedInstruction {
        self.instruction_from_idx(expr_idx).lift()
    }

    pub fn instruction_from_instruction_idx(&self, instr_idx: usize) -> MediumLevelILInstruction {
        MediumLevelILInstruction::new(self.to_owned(), unsafe {
            BNGetMediumLevelILIndexForInstruction(self.handle, instr_idx)
        })
    }

    pub fn lifted_instruction_from_instruction_idx(
        &self,
        instr_idx: usize,
    ) -> MediumLevelILLiftedInstruction {
        self.instruction_from_instruction_idx(instr_idx).lift()
    }

    pub fn instruction_count(&self) -> usize {
        unsafe { BNGetMediumLevelILInstructionCount(self.handle) }
    }

    pub fn ssa_form(&self) -> MediumLevelILFunction {
        let ssa = unsafe { BNGetMediumLevelILSSAForm(self.handle) };
        assert!(!ssa.is_null());
        MediumLevelILFunction { handle: ssa }
    }

    pub fn get_function(&self) -> Ref<Function> {
        unsafe {
            let func = BNGetMediumLevelILOwnerFunction(self.handle);
            Function::from_raw(func)
        }
    }

    pub fn basic_blocks(&self) -> Array<BasicBlock<MediumLevelILBlock>> {
        let mut count = 0;
        let blocks = unsafe { BNGetMediumLevelILBasicBlockList(self.handle, &mut count) };
        let context = MediumLevelILBlock {
            function: self.to_owned(),
        };

        unsafe { Array::new(blocks, count, context) }
    }

    pub fn get_var_definitions<'a>(&'a self, var: &Variable) -> MediumLevelILInstructionList<'a> {
        let mut count = 0;
        let raw_instrs =
            unsafe { BNGetMediumLevelILVariableDefinitions(self.handle, &var.raw(), &mut count) };
        assert!(!raw_instrs.is_null());
        let instrs = unsafe { core::slice::from_raw_parts(raw_instrs, count) };
        MediumLevelILInstructionList {
            mlil: self,
            ptr: raw_instrs,
            instr_idxs: instrs.iter(),
        }
    }

    pub fn create_user_stack_var<'a, S: BnStrCompatible, C: Into<Conf<&'a Type>>>(
        self,
        offset: i64,
        var_type: C,
        name: S,
    ) {
        let var_type = var_type.into();
        let mut raw_var_type: BNTypeWithConfidence = var_type.into();
        let name = name.into_bytes_with_nul();
        unsafe {
            BNCreateUserStackVariable(
                self.get_function().handle,
                offset,
                &mut raw_var_type,
                name.as_ref().as_ptr() as *const c_char,
            )
        }
    }

    pub fn delete_user_stack_var(self, offset: i64) {
        unsafe { BNDeleteUserStackVariable(self.get_function().handle, offset) }
    }

    pub fn create_user_var<'a, S: BnStrCompatible, C: Into<Conf<&'a Type>>>(
        &self,
        var: &Variable,
        var_type: C,
        name: S,
        ignore_disjoint_uses: bool,
    ) {
        let var_type = var_type.into();
        let raw_var_type: BNTypeWithConfidence = var_type.into();
        let name = name.into_bytes_with_nul();
        unsafe {
            BNCreateUserVariable(
                self.get_function().handle,
                &var.raw(),
                &raw_var_type as *const _ as *mut _,
                name.as_ref().as_ptr() as *const _,
                ignore_disjoint_uses,
            )
        }
    }

    pub fn delete_user_var(&self, var: &Variable) {
        unsafe { BNDeleteUserVariable(self.get_function().handle, &var.raw()) }
    }

    pub fn is_var_user_defined(&self, var: &Variable) -> bool {
        unsafe { BNIsVariableUserDefined(self.get_function().handle, &var.raw()) }
    }

    /// Allows the user to specify a PossibleValueSet value for an MLIL
    /// variable at its definition site.
    ///
    /// .. warning:: Setting the variable value, triggers a reanalysis of the
    /// function and allows the dataflow to compute and propagate values which
    /// depend on the current variable. This implies that branch conditions
    /// whose values can be determined statically will be computed, leading to
    /// potential branch elimination at the HLIL layer.
    ///
    /// * `var` - Variable for which the value is to be set
    /// * `addr` - Address of the definition site of the variable
    /// * `value` - Informed value of the variable
    ///
    /// # Example
    /// ```no_run
    /// # use binaryninja::mlil::MediumLevelILFunction;
    /// # use binaryninja::types::PossibleValueSet;
    /// # let mlil_fun: MediumLevelILFunction = todo!();
    /// let (mlil_var, arch_addr, _val) = mlil_fun.user_var_values().all().next().unwrap();
    /// let def_address = arch_addr.address;
    /// let var_value = PossibleValueSet::ConstantValue{value: 5};
    /// mlil_fun.set_user_var_value(&mlil_var, def_address, var_value).unwrap();
    /// ```
    pub fn set_user_var_value(
        &self,
        var: &Variable,
        addr: u64,
        value: PossibleValueSet,
    ) -> Result<(), ()> {
        let Some(_def_site) = self
            .get_var_definitions(var)
            .find(|def| def.address == addr)
        else {
            // Error "No definition for Variable found at given address"
            return Err(());
        };
        let function = self.get_function();
        let def_site = BNArchitectureAndAddress {
            arch: function.arch().0,
            address: addr,
        };
        let value = value.into_raw();

        unsafe { BNSetUserVariableValue(function.handle, &var.raw(), &def_site, value.as_ffi()) }
        Ok(())
    }

    /// Clears a previously defined user variable value.
    ///
    /// * `var` - Variable for which the value was informed
    /// * `def_addr` - Address of the definition site of the variable
    pub fn clear_user_var_value(&self, var: &Variable, addr: u64) -> Result<(), ()> {
        let Some(_var_def) = self
            .get_var_definitions(var)
            .find(|site| site.address == addr)
        else {
            //error "Could not get definition for Variable"
            return Err(());
        };

        let function = self.get_function();
        let def_site = BNArchitectureAndAddress {
            arch: function.arch().0,
            address: addr,
        };

        unsafe { BNClearUserVariableValue(function.handle, &var.raw(), &def_site) };
        Ok(())
    }

    /// Returns a map of current defined user variable values.
    /// Returns a Map of user current defined user variable values and their definition sites.
    pub fn user_var_values(&self) -> UserVariableValues {
        let mut count = 0;
        let function = self.get_function();
        let var_values = unsafe { BNGetAllUserVariableValues(function.handle, &mut count) };
        assert!(!var_values.is_null());
        UserVariableValues {
            vars: core::ptr::slice_from_raw_parts(var_values, count),
        }
    }

    /// Clear all user defined variable values.
    pub fn clear_user_var_values(&self) -> Result<(), ()> {
        for (var, arch_and_addr, _value) in self.user_var_values().all() {
            self.clear_user_var_value(&var, arch_and_addr.address)?;
        }
        Ok(())
    }

    pub fn create_auto_stack_var<'a, T: Into<Conf<&'a Type>>, S: BnStrCompatible>(
        &self,
        offset: i64,
        var_type: T,
        name: S,
    ) {
        let var_type: Conf<&Type> = var_type.into();
        let mut var_type = var_type.into();
        let name = name.into_bytes_with_nul();
        let name_c_str = name.as_ref();
        unsafe {
            BNCreateAutoStackVariable(
                self.get_function().handle,
                offset,
                &mut var_type,
                name_c_str.as_ptr() as *const c_char,
            )
        }
    }

    pub fn delete_auto_stack_var(&self, offset: i64) {
        unsafe { BNDeleteAutoStackVariable(self.get_function().handle, offset) }
    }

    pub fn create_auto_var<'a, S: BnStrCompatible, C: Into<Conf<&'a Type>>>(
        &self,
        var: &Variable,
        var_type: C,
        name: S,
        ignore_disjoint_uses: bool,
    ) {
        let var_type: Conf<&Type> = var_type.into();
        let mut var_type = var_type.into();
        let name = name.into_bytes_with_nul();
        let name_c_str = name.as_ref();
        unsafe {
            BNCreateAutoVariable(
                self.get_function().handle,
                &var.raw(),
                &mut var_type,
                name_c_str.as_ptr() as *const c_char,
                ignore_disjoint_uses,
            )
        }
    }

    /// Returns a list of ILReferenceSource objects (IL xrefs or cross-references)
    /// that reference the given variable. The variable is a local variable that can be either on the stack,
    /// in a register, or in a flag.
    /// This function is related to get_hlil_var_refs(), which returns variable references collected
    /// from HLIL. The two can be different in several cases, e.g., multiple variables in MLIL can be merged
    /// into a single variable in HLIL.
    ///
    /// * `var` - Variable for which to query the xref
    ///
    /// # Example
    /// ```no_run
    /// # use binaryninja::mlil::MediumLevelILFunction;
    /// # use binaryninja::types::Variable;
    /// # let mlil_fun: MediumLevelILFunction = todo!();
    /// # let mlil_var: Variable = todo!();
    /// let instr = mlil_fun.var_refs(&mlil_var).get(0).expr();
    /// ```
    pub fn var_refs(&self, var: &Variable) -> Array<ILReferenceSource> {
        let mut count = 0;
        let refs = unsafe {
            BNGetMediumLevelILVariableReferences(
                self.get_function().handle,
                &mut var.raw(),
                &mut count,
            )
        };
        assert!(!refs.is_null());
        unsafe { Array::new(refs, count, self.to_owned()) }
    }

    /// Returns a list of variables referenced by code in the function ``func``,
    /// of the architecture ``arch``, and at the address ``addr``. If no function is specified, references from
    /// all functions and containing the address will be returned. If no architecture is specified, the
    /// architecture of the function will be used.
    /// This function is related to get_hlil_var_refs_from(), which returns variable references collected
    /// from HLIL. The two can be different in several cases, e.g., multiple variables in MLIL can be merged
    /// into a single variable in HLIL.
    ///
    /// * `addr` - virtual address to query for variable references
    /// * `length` - optional length of query
    /// * `arch` - optional architecture of query
    pub fn var_refs_from(
        &self,
        addr: u64,
        length: Option<u64>,
        arch: Option<CoreArchitecture>,
    ) -> Array<VariableReferenceSource> {
        let function = self.get_function();
        let arch = arch.unwrap_or_else(|| function.arch());
        let mut count = 0;

        let refs = if let Some(length) = length {
            unsafe {
                BNGetMediumLevelILVariableReferencesInRange(
                    function.handle,
                    arch.0,
                    addr,
                    length,
                    &mut count,
                )
            }
        } else {
            unsafe {
                BNGetMediumLevelILVariableReferencesFrom(function.handle, arch.0, addr, &mut count)
            }
        };
        assert!(!refs.is_null());
        unsafe { Array::new(refs, count, self.to_owned()) }
    }
}

impl ToOwned for MediumLevelILFunction {
    type Owned = Ref<Self>;

    fn to_owned(&self) -> Self::Owned {
        unsafe { RefCountable::inc_ref(self) }
    }
}

unsafe impl RefCountable for MediumLevelILFunction {
    unsafe fn inc_ref(handle: &Self) -> Ref<Self> {
        Ref::new(Self {
            handle: BNNewMediumLevelILFunctionReference(handle.handle),
        })
    }

    unsafe fn dec_ref(handle: &Self) {
        BNFreeMediumLevelILFunction(handle.handle);
    }
}

impl core::fmt::Debug for MediumLevelILFunction {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "<mlil func handle {:p}>", self.handle)
    }
}

#[derive(Clone, Debug)]
pub struct MediumLevelILInstructionList<'a> {
    mlil: &'a MediumLevelILFunction,
    ptr: *mut usize,
    instr_idxs: core::slice::Iter<'a, usize>,
}

impl Drop for MediumLevelILInstructionList<'_> {
    fn drop(&mut self) {
        unsafe { BNFreeILInstructionList(self.ptr) };
    }
}

impl Iterator for MediumLevelILInstructionList<'_> {
    type Item = MediumLevelILInstruction;

    fn next(&mut self) -> Option<Self::Item> {
        self.instr_idxs
            .next()
            .map(|i| self.mlil.instruction_from_instruction_idx(*i))
    }
}

impl DoubleEndedIterator for MediumLevelILInstructionList<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.instr_idxs
            .next_back()
            .map(|i| self.mlil.instruction_from_instruction_idx(*i))
    }
}

impl ExactSizeIterator for MediumLevelILInstructionList<'_> {}
impl core::iter::FusedIterator for MediumLevelILInstructionList<'_> {}

/////////////////////////
// FunctionGraphType

pub type FunctionGraphType = binaryninjacore_sys::BNFunctionGraphType;

/////////////////////////
// ILReferenceSource

pub struct ILReferenceSource {
    mlil: Ref<MediumLevelILFunction>,
    _func: Ref<Function>,
    _arch: CoreArchitecture,
    addr: u64,
    type_: FunctionGraphType,
    expr_id: usize,
}

impl ILReferenceSource {
    unsafe fn from_raw(value: BNILReferenceSource, mlil: Ref<MediumLevelILFunction>) -> Self {
        Self {
            mlil,
            _func: Function::from_raw(value.func),
            _arch: CoreArchitecture::from_raw(value.arch),
            addr: value.addr,
            type_: value.type_,
            expr_id: value.exprId,
        }
    }
    pub fn addr(&self) -> u64 {
        self.addr
    }
    pub fn graph_type(&self) -> FunctionGraphType {
        self.type_
    }
    pub fn expr(&self) -> MediumLevelILInstruction {
        self.mlil.instruction_from_idx(self.expr_id)
    }
}

impl CoreArrayProvider for ILReferenceSource {
    type Raw = BNILReferenceSource;
    type Context = Ref<MediumLevelILFunction>;
    type Wrapped<'a> = Self;
}
unsafe impl CoreArrayProviderInner for ILReferenceSource {
    unsafe fn free(raw: *mut Self::Raw, count: usize, _context: &Self::Context) {
        BNFreeILReferences(raw, count)
    }
    unsafe fn wrap_raw<'a>(raw: &'a Self::Raw, context: &'a Self::Context) -> Self::Wrapped<'a> {
        Self::from_raw(*raw, context.to_owned())
    }
}

/////////////////////////
// VariableReferenceSource

pub struct VariableReferenceSource {
    var: Variable,
    source: ILReferenceSource,
}

impl VariableReferenceSource {
    pub fn variable(&self) -> &Variable {
        &self.var
    }
    pub fn source(&self) -> &ILReferenceSource {
        &self.source
    }
}

impl CoreArrayProvider for VariableReferenceSource {
    type Raw = BNVariableReferenceSource;
    type Context = Ref<MediumLevelILFunction>;
    type Wrapped<'a> = Self;
}

unsafe impl CoreArrayProviderInner for VariableReferenceSource {
    unsafe fn free(raw: *mut Self::Raw, count: usize, _context: &Self::Context) {
        BNFreeVariableReferenceSourceList(raw, count)
    }
    unsafe fn wrap_raw<'a>(raw: &'a Self::Raw, context: &'a Self::Context) -> Self::Wrapped<'a> {
        Self {
            var: Variable::from_raw(raw.var),
            source: ILReferenceSource::from_raw(raw.source, context.to_owned()),
        }
    }
}
