use forge_code::Code;
use forge_domain::ActiveFiles;

pub struct ForgeAllIdes {
    ides: Vec<IDEs>,
}

enum IDEs {
    VsCode(Code),
}

impl ForgeAllIdes {
    pub fn new<T: ToString>(cwd: T) -> Self {
        let ides: Vec<IDEs> = vec![IDEs::VsCode(Code::from(cwd.to_string()))];
        Self { ides }
    }
}

#[async_trait::async_trait]
impl ActiveFiles for ForgeAllIdes {
    async fn active_files(&self) -> anyhow::Result<Vec<String>> {
        let mut files = vec![];
        for ide in &self.ides {
            if let Ok(ide_files) = ide.active_files().await {
                files.extend(ide_files);
            }
        }
        if !files.is_empty() {
            files = vec![vec!["The active files are:".to_string()], files]
                .into_iter()
                .flatten()
                .collect();
        };
        Ok(files)
    }
}

#[async_trait::async_trait]
impl ActiveFiles for IDEs {
    async fn active_files(&self) -> anyhow::Result<Vec<String>> {
        match self {
            IDEs::VsCode(ide) => ide.active_files().await,
        }
    }
}
