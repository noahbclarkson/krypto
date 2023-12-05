
// The decision algorithm for making decisions based on the R-matrix.
pub enum RMatrixDecisionAlgorithm {
    // Sums all of the R-matrix predictions for the next [depth] periods. 
    // The greatest sum is the decison if it is greater than the minimum strength (configurable).
    // If no sum is greater than the minimum strength, the decision is to do nothing.
    SUM,
    // Uses CMA-ES to find the best weights for the R-matrix predictions for the next [depth] periods.
    // The greatest sum is the decison if it is greater than the minimum strength (configurable).
    // If no sum is greater than the minimum strength, the decision is to do nothing.
    CMAES,
}