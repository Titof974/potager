use std::fmt;

use crate::{blob::Blob, Manifest, RequestChunkUpload};

struct Registry {
    blobs: Vec<Blob>,
    manifests: Vec<Manifest>,
    request_chunk_uploads: Vec<RequestChunkUpload>,
}

trait RegistryOperations  {
    fn add_blob(&mut self, blob: Blob) -> Result<(), BlobNotFound>;
    fn get_blob_by_digest(&self, digest: String) -> Option<Blob>;
    fn get_manifest_by_digest(&self, digest: String) -> Option<Manifest>;
    fn get_manifest_by_tag(&self, tag: String) -> Option<Manifest>;
}

impl RegistryOperations for Registry {
    fn add_blob(&mut self, blob: Blob) -> Result<(), BlobNotFound> {
        if self.blobs.iter().any(|_blob| _blob.digest.eq(&blob.clone().digest)) {
            return Err(BlobNotFound{ blob });
        }
        self.blobs.push(blob.clone());
        Ok(())
    }

    fn get_blob_by_digest(&self, digest: String) -> Option<Blob> {
        self.blobs.iter().find(|_blob| _blob.digest.eq(&digest)).cloned()
    }

    fn get_manifest_by_digest(&self, digest: String) -> Option<Manifest> {
        self.manifests.iter().find(|_manifest| _manifest.digest.clone().expect("Can't retrieve manifest").eq(&digest)).cloned()
    }

    fn get_manifest_by_tag(&self, tag: String) -> Option<Manifest> {
        self.manifests.iter().find(|_manifest| 
           _manifest.tags.clone().expect("Can't retrieve tags from manifest").iter().any(|_tag| _tag.eq(&tag))
        ).cloned()
    }
}

#[derive(Debug, Clone)]
struct BlobNotFound {
    blob: Blob
}

impl fmt::Display for BlobNotFound {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Blob {} not found", self.blob.digest)
    }
}