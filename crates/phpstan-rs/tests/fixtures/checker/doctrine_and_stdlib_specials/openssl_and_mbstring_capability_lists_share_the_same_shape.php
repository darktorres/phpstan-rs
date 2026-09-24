<?php
            /**
             * @return non-empty-list<non-empty-string>
             */
            function ciphers() {
                return openssl_get_cipher_methods();
            }
            /**
             * @return non-empty-list<non-empty-string>
             */
            function encodings() {
                return mb_list_encodings();
            }
        