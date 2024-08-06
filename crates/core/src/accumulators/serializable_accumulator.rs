use arrow::array::{Array, ArrayRef};
use datafusion::functions_aggregate::array_agg::ArrayAggAccumulator;
use datafusion_common::{Result, ScalarValue};
use datafusion_expr::Accumulator;
use serde::{Deserialize, Serialize};

use super::serialize::SerializableScalarValue;

pub trait SerializableAccumulator {
    fn serialize(&mut self) -> Result<String>;
    fn deserialize(bytes: String) -> Result<Box<dyn Accumulator>>;
}

#[derive(Debug, Serialize, Deserialize)]
struct SerializableArrayAggState {
    state: Vec<SerializableScalarValue>,
}

impl SerializableAccumulator for ArrayAggAccumulator {
    fn serialize(&mut self) -> Result<String> {
        let state = self.state()?;
        let serializable_state = SerializableArrayAggState {
            state: state
                .into_iter()
                .map(SerializableScalarValue::from)
                .collect(),
        };
        Ok(serde_json::to_string(&serializable_state).unwrap())
    }

    fn deserialize(bytes: String) -> Result<Box<dyn Accumulator>> {
        let serializable_state: SerializableArrayAggState =
            serde_json::from_str(bytes.as_str()).unwrap();
        let state: Vec<ScalarValue> = serializable_state
            .state
            .into_iter()
            .map(ScalarValue::from)
            .collect();

        // Infer the datatype from the first element of the state
        let datatype = if let Some(ScalarValue::List(list)) = state.first() {
            list.data_type().clone()
        } else {
            return Err(datafusion_common::DataFusionError::Internal(
                "Invalid state for ArrayAggAccumulator".to_string(),
            ));
        };

        let mut acc = ArrayAggAccumulator::try_new(&datatype)?;

        // Convert ScalarValue to ArrayRef for merge_batch
        let arrays: Vec<ArrayRef> = state
            .into_iter()
            .filter_map(|s| {
                if let ScalarValue::List(list) = s {
                    Some(list.values().clone())
                } else {
                    None
                }
            })
            .collect();

        acc.merge_batch(&arrays)?;

        Ok(Box::new(acc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Int32Array, StringArray};
    use arrow::datatypes::DataType;
    use std::sync::Arc;

    fn create_int32_array(values: Vec<Option<i32>>) -> ArrayRef {
        Arc::new(Int32Array::from(values)) as ArrayRef
    }

    fn create_string_array(values: Vec<Option<&str>>) -> ArrayRef {
        Arc::new(StringArray::from(values)) as ArrayRef
    }

    #[test]
    fn test_serialize_deserialize_int32() -> Result<()> {
        let mut acc = ArrayAggAccumulator::try_new(&DataType::Int32)?;
        acc.update_batch(&[create_int32_array(vec![Some(1), Some(2), Some(3)])])?;

        let serialized = SerializableAccumulator::serialize(&mut acc)?;
        let mut deserialized = ArrayAggAccumulator::deserialize(serialized)?;

        assert_eq!(acc.evaluate()?, deserialized.evaluate()?);
        Ok(())
    }

    #[test]
    fn test_serialize_deserialize_string() -> Result<()> {
        let mut acc = ArrayAggAccumulator::try_new(&DataType::Utf8)?;
        acc.update_batch(&[create_string_array(vec![
            Some("hello"),
            Some("world"),
            None,
        ])])?;

        let serialized = SerializableAccumulator::serialize(&mut acc)?;
        let mut deserialized = ArrayAggAccumulator::deserialize(serialized)?;

        assert_eq!(acc.evaluate()?, deserialized.evaluate()?);
        Ok(())
    }

    #[test]
    fn test_serialize_deserialize_empty() -> Result<()> {
        let mut acc = ArrayAggAccumulator::try_new(&DataType::Int32)?;

        let serialized = SerializableAccumulator::serialize(&mut acc)?;
        let result = ArrayAggAccumulator::deserialize(serialized);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Empty state"));
        Ok(())
    }

    #[test]
    fn test_serialize_deserialize_multiple_updates() -> Result<()> {
        let mut acc = ArrayAggAccumulator::try_new(&DataType::Int32)?;
        acc.update_batch(&[create_int32_array(vec![Some(1), Some(2)])])?;
        acc.update_batch(&[create_int32_array(vec![Some(3), Some(4)])])?;

        let serialized = SerializableAccumulator::serialize(&mut acc)?;
        let mut deserialized = ArrayAggAccumulator::deserialize(serialized)?;

        assert_eq!(acc.evaluate()?, deserialized.evaluate()?);
        Ok(())
    }

    #[test]
    fn test_serialize_deserialize_with_nulls() -> Result<()> {
        let mut acc = ArrayAggAccumulator::try_new(&DataType::Int32)?;
        acc.update_batch(&[create_int32_array(vec![Some(1), None, Some(3)])])?;

        let serialized = SerializableAccumulator::serialize(&mut acc)?;
        let mut deserialized = ArrayAggAccumulator::deserialize(serialized)?;

        assert_eq!(acc.evaluate()?, deserialized.evaluate()?);
        Ok(())
    }
}
