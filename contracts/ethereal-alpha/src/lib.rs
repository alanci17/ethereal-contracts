use scrypto::prelude::*;

#[blueprint]
mod alpha {
  enable_method_auth! {
    roles {
      zero => updatable_by: [];
      azero => updatable_by: [];
      omega => updatable_by: [];
      // usd => updatable_by: []; TODO RESTRICT
    },
    methods {
      to_nothing => restrict_to: [zero];
      aa_rope => PUBLIC; // TODO restrict?
      set_app_addrs => restrict_to: [zero, azero];
      get_app_addrs => PUBLIC;
      prove_alpha => restrict_to: [omega];
      make_azero => restrict_to: [omega];
      set_dao_addr => restrict_to: [zero];
    }
  }

  struct Alpha {
    dao_addr: ComponentAddress,
    power_zero: ResourceAddress,

    power_alpha: Vault,
    power_azero: ResourceAddress, // alpha zero, zero of alpha
    
    // usd, eux, tri
    app_addrs: (ComponentAddress, ComponentAddress, ComponentAddress),
  }

  impl Alpha {
    pub fn from_nothing(
      dao_addr: ComponentAddress, power_zero: ResourceAddress, 
      power_omega: ResourceAddress, power_alpha: Bucket, power_azero: ResourceAddress,
      usd_addr: ComponentAddress, eux_addr: ComponentAddress, tri_addr: ComponentAddress,
      bang: ComponentAddress
    ) -> ComponentAddress {
      // power azero is passed in
      // dao script is deferred to for all the braiding
      // despite the layers being one step down the same, really
      

      Self {
        dao_addr,
        power_zero,

        power_alpha: Vault::with_bucket(power_alpha),
        power_azero,

        app_addrs: (usd_addr, eux_addr, tri_addr),
      }
      .instantiate()
      .prepare_to_globalize(OwnerRole::None)
      .roles(
        roles!(
          zero => rule!(require(power_zero));
          azero => rule!((require(power_azero)));
          omega => rule!((require(power_omega)));
        )
      )
      .metadata(
        metadata!(
          roles {
            metadata_setter => rule!(require(power_zero));
            metadata_setter_updater => rule!(deny_all);
            metadata_locker => rule!(deny_all);
            metadata_locker_updater => rule!(deny_all);
          },
          init {
            "dapp_definition" =>
              GlobalAddress::from(bang), updatable;
            "tags" => vec!["ethereal-dao".to_owned(), 
              "alpha".to_owned()], updatable;
          }
        )
      )
      .globalize()
      .address()
    }

    pub fn to_nothing(&mut self) -> Bucket {
      self.power_alpha.take_all()
    }

    // TODO: auth? is it worth guarding against someone 
    // donating their EUXLP here? like it makes treasury add liquidity 
    // but is that a bad thing? coung be an add high type situation
    // that makes the treasure take an L on the real it holds
    // but equivalently they can probably just swap
    // and it would be just as effective, if done with side that moves liquidity
    //
    // honestly don't see it being a problem: TODO ask vex
    //
    // automatically pairs it with treasury REAL
    pub fn aa_rope(&mut self, mut input: Bucket) {
      info!("aa_rope IN"); 

      // no check if it's euxlp, but if it isn't, it explodes HERE
      let dao: Global<AnyComponent> = self.dao_addr.into();

      let (_, delta_ca, _) = 
        dao.call_raw::<(ComponentAddress, ComponentAddress, ComponentAddress)>(
          "get_branch_addrs", scrypto_args!()
        );
      let delta: Global<AnyComponent> = delta_ca.into();
      
      // token boosted POL acquisition
      let aaboo = self.power_alpha.as_fungible().authorize_with_amount(dec!(1), || { 
        let (real, rem) = delta.call_raw::<(Option<Bucket>, Option<Bucket>)>
          ("aa_tap", scrypto_args!());
        
        if let Some(r) = rem {
          input.put(r);
        };

        real
      });

      // if no real allocated to AA, put EUXLP in treasury
      // and return early
      if let Some(real) = aaboo {
        // assumes order of REAL / EUXLP
        // HERE
        let tri: Global<AnyComponent> = self.app_addrs.2.into();
        // TODO: minimum price at which it adds it
        // ^ derive from avg stake value or something
        let spot = 
          tri.call_raw::<Decimal>("spot_price", scrypto_args!());

        // if spot is over a minimum price 
        // TODO FIX
        if spot > dec!("0.5") {
          let (tlp, remainder) = 
          tri.call_raw::<(Bucket, Option<Bucket>)>("add_liquidity", scrypto_args!(real, input));

          info!("aa_rope OUT"); 

          self.power_alpha.as_fungible().authorize_with_amount(dec!(1), || {
            delta.call_raw::<()>
              ("aa_out", scrypto_args!(remainder));
            delta.call_raw::<()>
              ("deposit", scrypto_args!(tlp));
          });
        } else {

        self.power_alpha.as_fungible().authorize_with_amount(dec!(1), || {
          delta.call_raw::<()>
              ("aa_out", scrypto_args!(Some(input)));
          });
        }
      } else {
        self.power_alpha.as_fungible().authorize_with_amount(dec!(1), || {
          delta.call_raw::<()>
            ("aa_out", scrypto_args!(Some(input)));
        });
      }
    }

    pub fn get_app_addrs(&self) -> (ComponentAddress, ComponentAddress, ComponentAddress) {
      self.app_addrs
    }

    pub fn set_app_addrs(&mut self, new: (ComponentAddress, ComponentAddress, ComponentAddress)) {
      self.app_addrs = new;
    }

    pub fn set_dao_addr(&mut self, new: ComponentAddress) {
      self.dao_addr = new;
    }

    // pupeteer alpha by omega
    pub fn prove_alpha(&self) -> FungibleProof {
      self.power_alpha.as_fungible().create_proof_of_amount(dec!(1))
    }

    pub fn make_azero(&mut self) -> Bucket {
      let rm = ResourceManager::from(self.power_azero);
      self.power_alpha.as_fungible().authorize_with_amount(dec!(1), || rm.mint(dec!(1)))
    }
  }
}