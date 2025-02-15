use forge_app::Infrastructure;

use crate::conversation::InMemoryWorkflowRepository;
use crate::env::ForgeEnvironmentService;
use crate::file_read::ForgeFileReadService;

pub struct ForgeInfra {
    _file_read_service: ForgeFileReadService,
    _environment_service: ForgeEnvironmentService,
    _workflow_repository: InMemoryWorkflowRepository,
}

impl ForgeInfra {
    pub fn new(restricted: bool) -> Self {
        Self {
            _file_read_service: ForgeFileReadService::new(),
            _environment_service: ForgeEnvironmentService::new(restricted),
            _workflow_repository: InMemoryWorkflowRepository::new(),
        }
    }
}

impl Infrastructure for ForgeInfra {
    type EnvironmentService = ForgeEnvironmentService;
    type FileReadService = ForgeFileReadService;

    fn environment_service(&self) -> &Self::EnvironmentService {
        &self._environment_service
    }

    fn file_read_service(&self) -> &Self::FileReadService {
        &self._file_read_service
    }
}
