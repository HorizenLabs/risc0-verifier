// Copyright Copyright 2024, Horizen Labs, Inc.
//
// SPDX-License-Identifier: Apache-2.0
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

pub trait CircuitCoreDef:
    risc0_zkp::adapter::CircuitCoreDef<risc0_zkp::field::baby_bear::BabyBear> + 'static
{
}

impl<T: risc0_zkp::adapter::CircuitCoreDef<risc0_zkp::field::baby_bear::BabyBear> + 'static>
    CircuitCoreDef for T
{
}

pub mod v1_0;

pub mod v1_1;

pub mod v1_2;
