use serde_json::Value;

/// 递归清理 JSON Schema 以符合 Gemini 接口要求
/// 
/// 1. [New] 展开 $ref 和 $defs: 将引用替换为实际定义，解决 Gemini 不支持 $ref 的问题
/// 2. 移除不支持的字段: $schema, additionalProperties, format, default, uniqueItems, validation fields
/// 3. 处理联合类型: ["string", "null"] -> "string"
/// 4. 将 type 字段的值转换为大写 (Gemini v1internal 要求)
/// 5. 移除数字校验字段: multipleOf, exclusiveMinimum, exclusiveMaximum 等
pub fn clean_json_schema(value: &mut Value) {
    // 0. 预处理：展开 $ref (Schema Flattening)
    if let Value::Object(map) = value {
        let mut defs = serde_json::Map::new();
        // 提取 $defs 或 definitions
        if let Some(Value::Object(d)) = map.remove("$defs") {
            defs.extend(d);
        }
        if let Some(Value::Object(d)) = map.remove("definitions") {
            defs.extend(d);
        }

        if !defs.is_empty() {
             // 递归替换引用
             flatten_refs(map, &defs);
        }
    }

    // 递归清理
    clean_json_schema_recursive(value);
}

/// 递归展开 $ref
fn flatten_refs(map: &mut serde_json::Map<String, Value>, defs: &serde_json::Map<String, Value>) {
    // 检查并替换 $ref
    if let Some(Value::String(ref_path)) = map.remove("$ref") {
        // 解析引用名 (例如 #/$defs/MyType -> MyType)
        let ref_name = ref_path.split('/').last().unwrap_or(&ref_path);
        
        if let Some(def_schema) = defs.get(ref_name) {
            // 将定义的内容合并到当前 map
            if let Value::Object(def_map) = def_schema {
                for (k, v) in def_map {
                    // 仅当当前 map 没有该 key 时才插入 (避免覆盖)
                    // 但通常 $ref 节点不应该有其他属性
                    map.entry(k.clone()).or_insert_with(|| v.clone());
                }
                
                // 递归处理刚刚合并进来的内容中可能包含的 $ref
                // 注意：这里可能会无限递归如果存在循环引用，但工具定义通常是 DAG
                flatten_refs(map, defs);
            }
        }
    }

    // 遍历子节点
    for (_, v) in map.iter_mut() {
        if let Value::Object(child_map) = v {
            flatten_refs(child_map, defs);
        } else if let Value::Array(arr) = v {
            for item in arr {
                if let Value::Object(item_map) = item {
                   flatten_refs(item_map, defs);
                }
            }
        }
    }
}

fn clean_json_schema_recursive(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // 1. 移除不支持的字段
            let fields_to_remove = [
                "$schema",
                // "$defs", "definitions", "$ref", // 这些已经在上面处理了
                "additionalProperties",
                "format",
                "default",
                "uniqueItems",
                "minLength",
                "maxLength",
                "minimum",
                "maximum",
                "exclusiveMinimum",
                "exclusiveMaximum",
                "multipleOf",
                "minItems",
                "maxItems",
                "pattern",
                "const",
                "minProperties",
                "maxProperties",
                "propertyNames",
                "patternProperties",
                "contains",
                "minContains",
                "maxContains",
                "if",
                "then",
                "else",
                "not",
                "anyOf", // Gemini 其实也对此有限制，尽量保留或简化
                "oneOf",
                "allOf"
            ];

            // 注意：Gemini 对 anyOf/oneOf 支持有限，可能需要进一步简化，
            // 但目前先只移除明确不支持的元数据关键字
            for field in fields_to_remove {
                // 对于 anyOf/oneOf/allOf，我们暂不移除，因为这涉及逻辑结构
                // 上面的 fields_to_remove 列表包含了它们，所以下面要加判断
                if field == "anyOf" || field == "oneOf" || field == "allOf" {
                    continue; 
                }
                map.remove(field);
            }
            
            // 移除特定不支持的字段 (硬编码列表中的)
            map.remove("$schema");
            map.remove("additionalProperties");
            map.remove("format");
            map.remove("default");
            map.remove("uniqueItems");
            // ... (上面的循环已经覆盖了大部分)

            // 2. 处理 type 字段 (Union Types -> Primary Type + Uppercase)
            if let Some(type_val) = map.get_mut("type") {
                match type_val {
                    Value::String(s) => {
                        *type_val = Value::String(s.to_uppercase());
                    }
                    Value::Array(arr) => {
                        // Handle ["string", "null"] -> select first non-null
                        let mut selected_type = "STRING".to_string(); // Default fallback
                        for item in arr {
                            if let Value::String(s) = item {
                                if s != "null" {
                                    selected_type = s.to_uppercase();
                                    break;
                                }
                            }
                        }
                        *type_val = Value::String(selected_type);
                    }
                    _ => {}
                }
            }

            // 3. 递归处理 properties, items, allOf, anyOf, oneOf 等
            for (key, v) in map.iter_mut() {
                // 特殊处理 properties 和 items 的子节点
                if key == "properties" {
                    if let Some(props_obj) = v.as_object_mut() {
                        for (_, prop_val) in props_obj.iter_mut() {
                            clean_json_schema_recursive(prop_val);
                        }
                    }
                } else if key == "items" {
                    clean_json_schema_recursive(v);
                } else if key == "allOf" || key == "anyOf" || key == "oneOf" {
                    if let Some(arr) = v.as_array_mut() {
                        for item in arr {
                            clean_json_schema_recursive(item);
                        }
                    }
                }
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                clean_json_schema_recursive(v);
            }
        }
        _ => {}
    }
}
