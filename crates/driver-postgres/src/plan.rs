#[cfg(test)]
mod plan_tests {
    use driver_api::PlanNode;

    #[test]
    fn test_deserialize_explain_plan() {
        let json_str = r#"
        {
          "Node Type": "Seq Scan",
          "Relation Name": "users",
          "Alias": "u",
          "Startup Cost": 0.00,
          "Total Cost": 12.50,
          "Plan Rows": 100,
          "Plan Width": 32,
          "Plans": [
            {
              "Node Type": "Hash Match",
              "Startup Cost": 10.00,
              "Total Cost": 20.00,
              "Plan Rows": 5,
              "Plan Width": 16
            }
          ]
        }
        "#;

        let node: PlanNode = serde_json::from_str(json_str).unwrap();
        assert_eq!(node.node_type, "Seq Scan");
        assert_eq!(node.relation_name.unwrap(), "users");
        assert_eq!(node.alias.unwrap(), "u");
        assert_eq!(node.startup_cost, 0.00);
        assert_eq!(node.total_cost, 12.50);
        assert_eq!(node.plan_rows, 100.0);
        assert_eq!(node.plan_width, 32);

        let children = node.plans.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].node_type, "Hash Match");
        assert_eq!(children[0].total_cost, 20.0);
    }
}
