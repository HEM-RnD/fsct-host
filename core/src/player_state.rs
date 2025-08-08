// Copyright 2025 HEM Sp. z o.o.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// This file is part of an implementation of Ferrum Streaming Control Technology™,
// which is subject to additional terms found in the LICENSE-FSCT.md file.

use crate::definitions::FsctStatus;
use crate::definitions::*;
use std::slice::Iter;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
}

// Iterator for track metadata remains
pub struct TrackMetadataIterator<'a> {
    metadata: &'a TrackMetadata,
    index: usize,
}

impl<'a> Iterator for TrackMetadataIterator<'a> {
    type Item = (FsctTextMetadata, &'a Option<String>);

    fn next(&mut self) -> Option<Self::Item> {
        let text_types = [FsctTextMetadata::CurrentTitle, FsctTextMetadata::CurrentAuthor,
            FsctTextMetadata::CurrentAlbum, FsctTextMetadata::CurrentGenre];
        if self.index < text_types.len() {
            let text_type = text_types[self.index];
            let text = self.metadata.get_text(text_type);
            self.index += 1;
            Some((text_type, text))
        } else {
            None
        }
    }
}

impl TrackMetadata {
    pub fn get_text(&self, text_type: FsctTextMetadata) -> &Option<String> {
        match text_type {
            FsctTextMetadata::CurrentTitle => &self.title,
            FsctTextMetadata::CurrentAuthor => &self.artist,
            FsctTextMetadata::CurrentAlbum => &self.album,
            FsctTextMetadata::CurrentGenre => &self.genre,
            _ => &None,
        }
    }

    pub fn get_mut_text(&mut self, text_type: FsctTextMetadata) -> &mut Option<String> {
        match text_type {
            FsctTextMetadata::CurrentTitle => &mut self.title,
            FsctTextMetadata::CurrentAuthor => &mut self.artist,
            FsctTextMetadata::CurrentAlbum => &mut self.album,
            FsctTextMetadata::CurrentGenre => &mut self.genre,
            _ => panic!("Unsupported text type"),
        }
    }

    pub fn iter(&self) -> TrackMetadataIterator {
        TrackMetadataIterator {
            metadata: self,
            index: 0,
        }
    }

    pub fn iter_id(&self) -> Iter<'static, FsctTextMetadata> {
        static TEXT_TYPES: [FsctTextMetadata; 4] = [FsctTextMetadata::CurrentTitle, FsctTextMetadata::CurrentAuthor,
            FsctTextMetadata::CurrentAlbum, FsctTextMetadata::CurrentGenre];
        TEXT_TYPES.iter()
    }
}

// PlayerState remains as a data structure
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlayerState {
    pub status: FsctStatus,
    pub timeline: Option<TimelineInfo>,
    pub texts: TrackMetadata,
}