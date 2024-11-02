//! Evaluation context.

use super::{Caches, GlobalRuntimeState};
use crate::{
    ast::FnCallHashes,
    expose_under_internals,
    func::calc_fn_hash,
    tokenizer::{is_valid_function_name, Token},
    types::Variant,
    Dynamic, Engine, FnArgsVec, FuncArgs, Position, RhaiResult, RhaiResultOf,
    Scope, StaticVec, ERR,
};
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// Context of a script evaluation process.
#[allow(dead_code)]
pub struct EvalContext<'a, 's, 'ps, 'g, 'c, 't> {
    /// The current [`Engine`].
    engine: &'a Engine,
    /// The current [`GlobalRuntimeState`].
    global: &'g mut GlobalRuntimeState,
    /// The current [caches][Caches], if available.
    caches: &'c mut Caches,
    /// The current [`Scope`].
    scope: &'s mut Scope<'ps>,
    /// The current bound `this` pointer, if any.
    this_ptr: Option<&'t mut Dynamic>,
}

impl<'a, 's, 'ps, 'g, 'c, 't> EvalContext<'a, 's, 'ps, 'g, 'c, 't> {
    /// Create a new [`EvalContext`].
    #[expose_under_internals]
    #[inline(always)]
    #[must_use]
    fn new(
        engine: &'a Engine,
        global: &'g mut GlobalRuntimeState,
        caches: &'c mut Caches,
        scope: &'s mut Scope<'ps>,
        this_ptr: Option<&'t mut Dynamic>,
    ) -> Self {
        Self { engine, global, caches, scope, this_ptr }
    }

    /// Call a function inside the call context with the provided arguments.
    #[inline]
    pub fn call_fn<T: Variant + Clone>(
        &self,
        fn_name: impl AsRef<str>,
        args: impl FuncArgs,
    ) -> RhaiResultOf<T> {
        let mut arg_values = StaticVec::new_const();
        args.parse(&mut arg_values);

        let args = &mut arg_values.iter_mut().collect::<FnArgsVec<_>>();

        self._call_fn_raw(fn_name, args, false, false, false).and_then(
            |result| {
                result.try_cast_result().map_err(|r| {
                    let result_type =
                        self.engine().map_type_name(r.type_name());
                    let cast_type = match std::any::type_name::<T>() {
                        typ if typ.contains("::") => {
                            self.engine.map_type_name(typ)
                        }
                        typ => typ,
                    };
                    ERR::ErrorMismatchOutputType(
                        cast_type.into(),
                        result_type.into(),
                        Position::NONE,
                    )
                    .into()
                })
            },
        )
    }

    /// Call a registered native Rust function inside the call context with the provided arguments.
    #[inline]
    pub fn call_native_fn<T: Variant + Clone>(
        &self,
        fn_name: impl AsRef<str>,
        args: impl FuncArgs,
    ) -> RhaiResultOf<T> {
        let mut arg_values = StaticVec::new_const();
        args.parse(&mut arg_values);

        let args = &mut arg_values.iter_mut().collect::<FnArgsVec<_>>();

        self._call_fn_raw(fn_name, args, true, false, false).and_then(
            |result| {
                result.try_cast_result().map_err(|r| {
                    let result_type =
                        self.engine().map_type_name(r.type_name());
                    let cast_type = match std::any::type_name::<T>() {
                        typ if typ.contains("::") => {
                            self.engine.map_type_name(typ)
                        }
                        typ => typ,
                    };
                    ERR::ErrorMismatchOutputType(
                        cast_type.into(),
                        result_type.into(),
                        Position::NONE,
                    )
                    .into()
                })
            },
        )
    }

    /// Call a function (native Rust or scripted) inside the call context.
    #[inline(always)]
    pub fn call_fn_raw(
        &self,
        fn_name: impl AsRef<str>,
        is_ref_mut: bool,
        is_method_call: bool,
        args: &mut [&mut Dynamic],
    ) -> RhaiResult {
        let name = fn_name.as_ref();
        let native_only = !is_valid_function_name(name);
        #[cfg(not(feature = "no_function"))]
        let native_only = native_only && !crate::parser::is_anonymous_fn(name);

        self._call_fn_raw(
            fn_name,
            args,
            native_only,
            is_ref_mut,
            is_method_call,
        )
    }

    /// Call a registered native Rust function inside the call context.
    #[inline(always)]
    pub fn call_native_fn_raw(
        &self,
        fn_name: impl AsRef<str>,
        is_ref_mut: bool,
        args: &mut [&mut Dynamic],
    ) -> RhaiResult {
        self._call_fn_raw(fn_name, args, true, is_ref_mut, false)
    }

    /// To call a function native to rust or scripted.
    fn _call_fn_raw(
        &self,
        fn_name: impl AsRef<str>,
        args: &mut [&mut Dynamic],
        native_only: bool,
        is_ref_mut: bool,
        is_method_call: bool,
    ) -> RhaiResult {
        let global = &mut self.global.clone();
        global.level += 1;

        let caches = &mut Caches::new();

        let fn_name = fn_name.as_ref();

        let op_token = Token::lookup_symbol_from_syntax(fn_name);

        let args_len = args.len();

        if native_only {
            self.engine
                .exec_native_fn_call(
                    global,
                    caches,
                    fn_name,
                    op_token.as_ref(),
                    calc_fn_hash(None, fn_name, args_len),
                    args,
                    is_ref_mut,
                    false,
                    Position::NONE,
                )
                .map(|(r, _)| r)
        } else {
            let hash = match is_method_call {
                #[cfg(not(feature = "no_function"))]
                true => FnCallHashes::from_script_and_native(
                    calc_fn_hash(None, fn_name, args_len - 1),
                    calc_fn_hash(None, fn_name, args_len),
                ),
                #[cfg(feature = "no_function")]
                true => FnCallHashes::from_native_only(calc_fn_hash(
                    None, fn_name, args_len,
                )),
                _ => FnCallHashes::from_hash(calc_fn_hash(
                    None, fn_name, args_len,
                )),
            };

            self.engine()
                .exec_fn_call(
                    global,
                    caches,
                    None,
                    fn_name,
                    op_token.as_ref(),
                    hash,
                    args,
                    is_ref_mut,
                    is_method_call,
                    Position::NONE,
                )
                .map(|(r, ..)| r)
        }
    }

    /// The current [`Engine`].
    #[inline(always)]
    #[must_use]
    pub const fn engine(&self) -> &'a Engine {
        self.engine
    }
    /// The current source.
    #[inline(always)]
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        self.global.source()
    }
    /// The current [`Scope`].
    #[inline(always)]
    #[must_use]
    pub const fn scope(&self) -> &Scope<'ps> {
        self.scope
    }
    /// Get a mutable reference to the current [`Scope`].
    #[inline(always)]
    #[must_use]
    pub fn scope_mut(&mut self) -> &mut Scope<'ps> {
        self.scope
    }
    /// Get an iterator over the current set of modules imported via `import` statements,
    /// in reverse order (i.e. modules imported last come first).
    #[cfg(not(feature = "no_module"))]
    #[inline(always)]
    pub fn iter_imports(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::Module)> {
        self.global.iter_imports()
    }
    /// Custom state kept in a [`Dynamic`].
    #[inline(always)]
    pub const fn tag(&self) -> &Dynamic {
        &self.global.tag
    }
    /// Mutable reference to the custom state kept in a [`Dynamic`].
    #[inline(always)]
    pub fn tag_mut(&mut self) -> &mut Dynamic {
        &mut self.global.tag
    }
    /// _(internals)_ The current [`GlobalRuntimeState`].
    /// Exported under the `internals` feature only.
    #[cfg(feature = "internals")]
    #[inline(always)]
    #[must_use]
    pub const fn global_runtime_state(&self) -> &GlobalRuntimeState {
        self.global
    }
    /// _(internals)_ Get a mutable reference to the current [`GlobalRuntimeState`].
    /// Exported under the `internals` feature only.
    #[cfg(feature = "internals")]
    #[inline(always)]
    #[must_use]
    pub fn global_runtime_state_mut(&mut self) -> &mut GlobalRuntimeState {
        self.global
    }
    /// Get an iterator over the namespaces containing definition of all script-defined functions.
    ///
    /// Not available under `no_function`.
    #[cfg(not(feature = "no_function"))]
    #[inline]
    pub fn iter_namespaces(&self) -> impl Iterator<Item = &crate::Module> {
        self.global.lib.iter().map(<_>::as_ref)
    }
    /// _(internals)_ The current set of namespaces containing definitions of all script-defined functions.
    /// Exported under the `internals` feature only.
    ///
    /// Not available under `no_function`.
    #[cfg(not(feature = "no_function"))]
    #[cfg(feature = "internals")]
    #[inline(always)]
    #[must_use]
    pub fn namespaces(&self) -> &[crate::SharedModule] {
        &self.global.lib
    }
    /// The current bound `this` pointer, if any.
    #[inline(always)]
    #[must_use]
    pub fn this_ptr(&self) -> Option<&Dynamic> {
        self.this_ptr.as_deref()
    }
    /// Mutable reference to the current bound `this` pointer, if any.
    #[inline(always)]
    #[must_use]
    pub fn this_ptr_mut(&mut self) -> Option<&mut Dynamic> {
        self.this_ptr.as_deref_mut()
    }
    /// The current nesting level of function calls.
    #[inline(always)]
    #[must_use]
    pub const fn call_level(&self) -> usize {
        self.global.level
    }

    /// Evaluate an [expression tree][crate::Expression] within this [evaluation context][`EvalContext`].
    ///
    /// # WARNING - Low Level API
    ///
    /// This function is very low level.  It evaluates an expression from an [`AST`][crate::AST].
    #[cfg(not(feature = "no_custom_syntax"))]
    #[inline(always)]
    pub fn eval_expression_tree(
        &mut self,
        expr: &crate::Expression,
    ) -> crate::RhaiResult {
        #[allow(deprecated)]
        self.eval_expression_tree_raw(expr, true)
    }
    /// Evaluate an [expression tree][crate::Expression] within this [evaluation context][`EvalContext`].
    ///
    /// The following option is available:
    ///
    /// * whether to rewind the [`Scope`] after evaluation if the expression is a [`StmtBlock`][crate::ast::StmtBlock]
    ///
    /// # WARNING - Unstable API
    ///
    /// This API is volatile and may change in the future.
    ///
    /// # WARNING - Low Level API
    ///
    /// This function is _extremely_ low level.  It evaluates an expression from an [`AST`][crate::AST].
    #[cfg(not(feature = "no_custom_syntax"))]
    #[deprecated = "This API is NOT deprecated, but it is considered volatile and may change in the future."]
    #[inline]
    pub fn eval_expression_tree_raw(
        &mut self,
        expr: &crate::Expression,
        rewind_scope: bool,
    ) -> crate::RhaiResult {
        let expr: &crate::ast::Expr = expr;
        let this_ptr = self.this_ptr.as_deref_mut();

        match expr {
            crate::ast::Expr::Stmt(stmts) => self.engine.eval_stmt_block(
                self.global,
                self.caches,
                self.scope,
                this_ptr,
                stmts.statements(),
                rewind_scope,
            ),
            _ => self.engine.eval_expr(
                self.global,
                self.caches,
                self.scope,
                this_ptr,
                expr,
            ),
        }
    }
}
