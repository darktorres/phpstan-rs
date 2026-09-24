<?php
            enum Status implements \Serializable {
                case Active;
                public function serialize(): ?string { return null; }
                public function unserialize($data): void {}
            }
        