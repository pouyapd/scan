use super::{
    BoolOp, ConstantValue, EqCompOp, Expression, Identifier, IntOp, IteOp, NegOp, NumCompOp,
    Real2IntOp, RealOp,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Property {
    /// the property's name, unique among all the properties of the model
    pub(crate) name: Identifier,
    /// the state-set formula
    pub(crate) expression: PropertyExpression,
    /// an optional comment
    #[serde(skip)]
    pub(crate) _comment: String,
}

#[derive(Deserialize)]
#[serde(untagged, rename_all = "kebab-case")]
pub(crate) enum PropertyExpression {
    /// constant value
    ConstantValue(ConstantValue),
    /// constant or variable reference; has the type of the constant or variable;
    /// if this type is a bounded type with base type t, then it has type t instead;
    /// constant expression iff it is a constant reference
    Identifier(Identifier),
    /// if-then-else: computes if if then then else else
    IfThenElse {
        /// the result type is the type of then if that is assignable from the type of else,
        /// or the type of else if that is assignable from the type of then
        op: IteOp,
        /// the condition; type bool
        r#if: Box<PropertyExpression>,
        /// the consequence
        r#then: Box<PropertyExpression>,
        /// the alternative
        r#else: Box<PropertyExpression>,
    },
    /// disjunction / conjunction: computes left ∨ right / left ∧ right
    Bool {
        /// result type is bool
        op: BoolOp,
        /// the left operand; type bool
        left: Box<PropertyExpression>,
        /// the right operand; type bool
        right: Box<PropertyExpression>,
    },
    /// negation: computes ¬exp
    Neg {
        /// result type is bool
        op: NegOp,
        /// the single operand; type bool
        exp: Box<PropertyExpression>,
    },
    /// equality comparison: computes left = right / left ≠ right
    EqComp {
        /// result type is bool; left and right must be assignable to some common type
        op: EqCompOp,
        /// the left operand
        left: Box<PropertyExpression>,
        /// the right operand
        right: Box<PropertyExpression>,
    },
    /// numeric comparison: computes left < right / left ≤ right
    NumComp {
        /// result type is bool
        op: NumCompOp,
        /// the left operand; numeric type
        left: Box<PropertyExpression>,
        /// the right operand; numeric type
        right: Box<PropertyExpression>,
    },
    /// addition / subtraction / multiplication / modulo:
    IntOp {
        /// result type is int (if left and right are both assignable to int) or real
        op: IntOp,
        /// the left operand; numeric type (must be int if op is "%")
        left: Box<PropertyExpression>,
        /// the right operand; numeric type (must be int if op is "%")
        right: Box<PropertyExpression>,
    },
    /// division / exponentiation / logarithm:
    RealOp {
        /// result type is real (division is real division, no truncation for integers)
        op: RealOp,
        /// the left operand; numeric type
        left: Box<PropertyExpression>,
        /// the right operand; numeric type
        right: Box<PropertyExpression>,
    },
    /// floor / ceiling: computes ⌊exp⌋ / ⌈exp⌉
    #[allow(dead_code)]
    Real2IntOp {
        /// result type is int
        op: Real2IntOp,
        /// the single operand; numeric type
        exp: Box<PropertyExpression>,
    },
    /// until / weak until
    #[allow(dead_code)]
    Until {
        /// result type is bool
        op: UntilOp,
        /// the left formula, type bool
        left: Box<PropertyExpression>,
        /// the right formula, type bool
        right: Box<PropertyExpression>,
        // "?step-bounds": PropertyInterval, // step bounds (number of edges taken) of type int
        /// and time bounds of numeric type, only allowed in timed models
        time_bounds: Option<PropertyInterval>,
        // "?reward-bounds": Array.of({ // and a conjunction of reward bounds
        //   "exp": Expression, // what to accumulate over steps and time for this subformula
        //   "accumulate": RewardAccumulation, // must not be empty
        //   "bounds": PropertyInterval // the bounds of numeric type
        // })
    },
}

#[derive(Deserialize)]
#[allow(dead_code)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct PropertyInterval {
    /// constant expression, must be present if upper is omitted
    lower: Option<Expression>,
    /// indicates whether the lower bound is exclusive (else inclusive);
    /// must not be present if lower is not present;
    /// if not present when lower is present, the value is false
    lower_exclusive: Option<bool>,
    /// constant expression, must be present if lower is omitted
    upper: Option<Expression>,
    /// indicates whether the upper bound is exclusive (else inclusive);
    /// must not be present if upper is not present;
    /// if not present when upper is present, the value is false
    upper_exclusive: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum UntilOp {
    #[serde(rename = "U")]
    Until,
    #[serde(rename = "W")]
    WeakUntil,
}
