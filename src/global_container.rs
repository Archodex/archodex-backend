use serde::{Deserialize, Serialize};

use crate::resource::ResourceId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct GlobalContainer {
    pub(crate) id: ResourceId,
    pub(crate) contains: ResourceId,
}
