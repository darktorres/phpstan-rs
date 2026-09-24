<?php
            class Recipient92 {}
            interface RecipientRepository92 {
                /**
                 * @return list<Recipient92>
                 */
                public function findAll(): array;
            }
            function make(): RecipientRepository92 {
                return new class implements RecipientRepository92 {
                    public function findAll(): array {
                        return [];
                    }
                };
            }
        