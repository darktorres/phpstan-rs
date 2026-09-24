<?php
            /**
             * @ORM\Entity(repositoryClass="App\Repo")
             * @ORM\Table(name="users", indexes={@ORM\Index(name="idx", columns={"a", "b"})})
             * @Route("/users/{id}", name="user_show", methods={"GET", "POST"}, requirements={"id": "\d+"})
             * @Assert\Choice(choices={1, 2.5, true, null}, strict=true,)
             * @Serializer\Type("array<string>")
             * @Custom(value=Foo::BAR, all=Foo::*, key = 'single')
             * @Empty()
             */
            class User {}
        