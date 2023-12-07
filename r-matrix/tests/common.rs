use proptest::{arbitrary::any, prelude::prop, strategy::Strategy};
use r_matrix::dataset::Dataset;

pub fn arb_dataset() -> impl Strategy<Value = Dataset> {
    let feature_count = any::<usize>().prop_map(|x| x % 10 + 1); // Ensures at least 1 feature, up to 10
    let label_count = any::<usize>().prop_map(|x| x % 10 + 1); // Ensures at least 1 label, up to 10

    (feature_count, label_count).prop_flat_map(|(fc, lc)| {
        let feature_vec = prop::collection::vec(any::<f64>(), fc);
        let label_vec = prop::collection::vec(any::<f64>(), lc);

        prop::collection::vec((any::<usize>(), feature_vec, label_vec), 0..50).prop_map(
            move |data| {
                let feature_names = (1..=fc)
                    .map(|i| format!("test_feature_{}", i))
                    .collect::<Vec<_>>();
                let label_names = (1..=lc).map(|i| format!("test_label_{}", i)).collect::<Vec<_>>();
                let mut builder = Dataset::builder();
                for (time, features, labels) in data.iter() {
                    builder.add_data_point(*time, features.clone(), labels.clone());
                }
                builder
                    .set_feature_names(feature_names)
                    .set_label_names(label_names);
                builder.build().unwrap()
            },
        )
    })
}
